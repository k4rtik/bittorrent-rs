#![recursion_limit = "1024"]

#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate log;
#[macro_use]
extern crate nom;

extern crate bip_bencode;
extern crate bip_metainfo;
extern crate bip_util;
extern crate bip_utracker;
extern crate chrono;
extern crate futures;
extern crate hyper;
extern crate pnet;
extern crate pnet_macros_support;
extern crate pretty_env_logger;
extern crate rustyline;
extern crate tokio_core;
extern crate url;

mod btclient;
mod packet;

mod errors {
    // Create the Error, ErrorKind, ResultExt, and Result types
    error_chain!{}
}

use bip_bencode::Bencode;
use bip_metainfo::MetainfoFile;
use bip_utracker::contact::CompactPeersV4;
use btclient::BTClient;
use chrono::{TimeZone, UTC};
use errors::*;
use hyper::Client;
use hyper::header::Connection;
use packet::peer_pkt::{MutablePeerHandshakePacket, MutablePeerMessagePacket};
use pnet::packet::Packet;
use rustyline::completion::FilenameCompleter;
use rustyline::{Config, CompletionType, Editor};
use rustyline::error::ReadlineError;
use tokio_core::io::{read, write_all};
use tokio_core::net::TcpStream;
use tokio_core::reactor::{Core, Timeout};
use url::Url;
use btclient::Torrent;

use std::fs::File;
use std::io::prelude::*;
use std::net::SocketAddr;
use std::str;
use std::string::String;
use std::thread;

const HISTORY_FILE: &'static str = ".rustyline.history";
const PEER_HANDSHAKE_STRUCT_SZ: usize = 68;
const PEER_REQ_PKT_SZ: usize = 14;

/// Print file list from the torrent.
fn print_files(bytes: &[u8]) -> Result<()> {
    let metainfo = MetainfoFile::from_bytes(bytes).unwrap();
    let info = metainfo.info();

    println!("File List:");
    println!("Size (bytes)\tPath");
    println!("------------\t----------------------------------------------");
    for file in info.files() {
        println!("{:12}\t{}",
                 file.length(),
                 file.paths().next().unwrap_or("<unknown>"));
    }
    Ok(())
}

/// Print general information about the torrent.
fn print_metainfo_overview(bytes: &[u8]) -> Result<()> {
    let metainfo = MetainfoFile::from_bytes(bytes).unwrap();
    let info = metainfo.info();
    let info_hash_hex = metainfo.info_hash()
        .as_ref()
        .iter()
        .map(|b| format!("{:02x}", b))
        .fold(String::new(), |mut acc, nex| {
            acc.push_str(&nex);
            acc
        });
    let utc_creation_date = metainfo.creation_date().map(|c| UTC.timestamp(c, 0));

    println!("------Metainfo File Overview-----");

    println!("InfoHash: {}", info_hash_hex);
    println!("Main Tracker: {:?}", metainfo.main_tracker());
    println!("Comment: {:?}", metainfo.comment());
    println!("Creator: {:?}", metainfo.created_by());
    println!("Creation Date: {:?}", utc_creation_date);

    println!("Directory: {:?}", info.directory());
    println!("Piece Length: {:?}", info.piece_length());
    println!("Number Of Pieces: {}", info.pieces().count());
    println!("Number Of Files: {}", info.files().count());
    println!("Total File Size: {}\n",
             info.files().fold(0, |acc, nex| acc + nex.length()));

    print_files(bytes)?;
    Ok(())
}

fn connect_to_tracker(metainfo: MetainfoFile,
                      peer_id: String,
                      port: u16)
                      -> Result<(Vec<String>, String)> {
    debug!("connecting to tracker: {:?}", metainfo.main_tracker());

    let info_hash = metainfo.info_hash();
    // TODO figure can this conversion to url-encoded form be done safely?
    let info_hash_str = unsafe { str::from_utf8_unchecked(info_hash.as_ref()) };

    // TODO this needs to be calculated based on what we have
    let total_len = metainfo.info().files().fold(0, |acc, nex| acc + nex.length());

    let mut url = Url::parse(metainfo.main_tracker().unwrap()).unwrap();
    url.query_pairs_mut()
        .append_pair("info_hash", info_hash_str)
        .append_pair("peer_id", &peer_id)
        .append_pair("port", &(port.to_string()))
        // TODO parametrize this
        .append_pair("uploaded", "0")
        // TODO parametrize this
        .append_pair("downloaded", "0")
        // TODO see note on total_len above
        .append_pair("left", &(total_len.to_string()))
        .append_pair("compact", "1")
        .append_pair("event", "started")
        .append_pair("supportcrypto", "0");
    trace!("URL {:?}", url);

    let client = Client::new();
    let mut res = client.get(url).header(Connection::close()).send().unwrap();
    let mut buffer = Vec::new();
    res.read_to_end(&mut buffer).unwrap();
    debug!("{:?}", res);
    let bencode = Bencode::decode(&buffer).unwrap();
    trace!("{:?}", bencode);
    let (_, peers) = CompactPeersV4::from_bytes(bencode.dict()
            .unwrap()
            .lookup("peers")
            .unwrap()
            .bytes()
            .unwrap())
        .unwrap();
    trace!("{:?}", peers);
    let mut ip_ports: Vec<String> = Vec::new();
    debug!("Peer list received:");
    for peer in peers.iter() {
        debug!("{:?}", peer);
        ip_ports.push(peer.to_string());
    }
    Ok((ip_ports, info_hash_str.to_string()))
}

fn send_keep_alive(mut stream: &TcpStream) -> Result<()> {
    let buf = vec![0; 1];
    match stream.write(&buf) {
        Ok(_) => Ok(()),
        Err(e) => bail!("Writing to stream  failed! {:?}", e),
    }
}

fn peer_connections(peer_ip_ports: Vec<String>, info_hash: &str, peer_id: String) -> Result<()> {
    for ip_port in peer_ip_ports {
        let peer_ip_port = ip_port.clone();
        let info_hash_clone = info_hash.to_string().clone();
        let peer_id_clone = peer_id.clone();
        thread::spawn(move || {
            fn run(pip: String, ih: String, pid: String) -> Result<()> {
                let mut l = Core::new().unwrap();
                // TODO setup timeout before handshake
                // let dur = Duration::from_secs(100);
                // let timeout = Timeout::new(dur, &l.handle()).unwrap();

                if let Ok(client) = handshake_peer(&mut l, pip, ih, pid)
                    .chain_err(|| "handshake failed") {
                    // debug!("Communicating with peer {:?}", peer_ip_port_cl);
                    // debug!("Starting thread timer...");
                    if let Ok((_, buf, amt)) =
                        l.run(read(client, vec![0; PEER_HANDSHAKE_STRUCT_SZ]))
                            .chain_err(|| "didn't receive handshake response from peer") {
                        if buf[0] == 19 &&
                           String::from_utf8_lossy(&buf[1..20]) == "BitTorrent protocol" &&
                           amt == 68 {
                            info!("handshake successful!");
                        } else {
                            info!("not a useful peer")
                        }
                    } else {
                        bail!("");
                    }
                } else {
                    bail!("");
                }
                Ok(())
            }
            if let Err(ref e) = run(peer_ip_port, info_hash_clone, peer_id_clone) {
                println!("error: {}", e);

                for e in e.iter().skip(1) {
                    println!("caused by: {}", e);
                }

                if let Some(backtrace) = e.backtrace() {
                    println!("backtrace: {:?}", backtrace);
                }
            }

            // TODO determine the below three parameters logically
            // let index = 0;
            // let begin = 0;
            // let length = 10;
            // match request_piece(&stream, index, begin, length) {
            //     Ok(buf_read) => {
            //         trace!("buf_read: {:?}", buf_read);
            //     }
            //     Err(_) => error!("Requesting piece failed!"),
            // }
            // send_keep_alive(&stream);
        });
    }
    Ok(())
}


fn request_piece(mut stream: &TcpStream, index: u32, begin: u32, length: u32) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; PEER_REQ_PKT_SZ];
    let mut peer_msg_pkt = MutablePeerMessagePacket::new(&mut buf).unwrap();
    peer_msg_pkt.set_len(13);
    peer_msg_pkt.set_id(6);
    let mut index_begin_length: Vec<u32> = Vec::new();
    index_begin_length.push(index);
    index_begin_length.push(begin);
    index_begin_length.push(length);
    let byte_arr = unsafe { std::mem::transmute::<Vec<u32>, Vec<u8>>(index_begin_length) };

    peer_msg_pkt.set_payload(&byte_arr);

    match stream.write(peer_msg_pkt.packet()) {
        Ok(_) => {
            let mut buf_read = vec![0; 2048];
            match stream.read(&mut buf_read) {
                Ok(bytes_read) => {
                    debug!("Bytes read: {:?}", bytes_read);
                    Ok(buf_read[..bytes_read].to_vec())
                }
                Err(e) => {
                    // error!("Read failed! {:?}", e);
                    bail!("Read failed! {:?}", e)
                }
            }
        }
        Err(e) => {
            bail!("Write to stream failed! {:?}", e)
            // Err("Write to stream failed!".to_owned())
        }
    }
}

fn handshake_peer(core: &mut Core,
                  peer_ip_port: String,
                  info_hash: String,
                  peer_id: String)
                  -> Result<TcpStream> {
    let mut buf = vec![0u8; PEER_HANDSHAKE_STRUCT_SZ];
    let mut ph = MutablePeerHandshakePacket::new(&mut buf).unwrap();
    ph.set_pstrlen("BitTorrent protocol".len() as u8);
    ph.set_pstr(&String::from("BitTorrent protocol").into_bytes());
    ph.set_reserved(&[0; 8]);
    ph.set_info_hash(info_hash.as_bytes());
    ph.set_peer_id(peer_id.as_bytes());
    let handle = core.handle();
    let client = TcpStream::connect(&peer_ip_port.parse::<SocketAddr>().unwrap(), &handle);
    let client = core.run(client).chain_err(|| "unable to connect to peer")?;
    let (client, _) = core.run(write_all(client, ph.packet())).unwrap();
    trace!("{:?}", client);
    Ok(client)
}

fn main() {
    pretty_env_logger::init();

    // See: https://brson.github.io/2016/11/30/starting-with-error-chain for explanation
    if let Err(ref e) = run() {
        println!("error: {}", e);

        for e in e.iter().skip(1) {
            println!("caused by: {}", e);
        }

        if let Some(backtrace) = e.backtrace() {
            println!("backtrace: {:?}", backtrace);
        }

        ::std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut btclient = BTClient::new();

    let config = Config::builder()
        .history_ignore_space(true)
        .completion_type(CompletionType::List)
        .build();
    let c = FilenameCompleter::new();
    let mut rl = Editor::with_config(config);
    rl.set_completer(Some(c));
    if rl.load_history(HISTORY_FILE).is_err() {
        info!("No previous history!");
    }

    loop {
        let readline = rl.readline("> ");
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_ref());

                let line = line.trim().split(' ').collect::<Vec<&str>>();

                match line[0] {
                    "help" | "h" => {
                        println!("Commands:
parse/p <torrent file path>      - show Metainfo File Overview
add/a <torrent file path>        - add a new torrent file for tracking
remove/r <torrent id>            - remove given torrent from tracking list
list/l                           - list torrents being tracked
showfiles/sf <torrent file path> - show files in the torrent
help/h                           - show this help");
                    }
                    "parse" | "p" => {
                        if line.len() != 2 {
                            error!("usage: parse <torrent file>");
                        } else {
                            let path = line[1];
                            debug!("MetainfoFile: {:?}", path);

                            match File::open(path) {
                                Ok(mut f) => {
                                    let mut bytes: Vec<u8> = Vec::new();
                                    f.read_to_end(&mut bytes).unwrap();
                                    print_metainfo_overview(&bytes)?;
                                }
                                Err(e) => error!("{:?}", e),
                            }
                        }
                    }
                    "add" | "a" => {
                        if line.len() != 2 {
                            error!("usage: add <torrent file>");
                        } else {
                            let path = line[1];
                            match File::open(path) {
                                Ok(mut f) => {
                                    let mut bytes: Vec<u8> = Vec::new();
                                    f.read_to_end(&mut bytes).unwrap();
                                    // TODO
                                    // create Torrent (parse metainfo file into our struct)
                                    // call BTClient->add()
                                    let t_file = MetainfoFile::from_bytes(&bytes).unwrap();
                                    let torrent_name = path.split('/').collect::<Vec<&str>>().pop().unwrap().split(".torrent").collect::<Vec<&str>>()[0];
                                    debug!("Torrent file {:?}", torrent_name);
                                    let torrent =
                                        Torrent::new(Url::parse(t_file.main_tracker()
                                                             .unwrap())
                                                         .unwrap(),
                                                     t_file.info().piece_length() as usize,
                                                     Vec::new());
                                    let result =
                                        connect_to_tracker(MetainfoFile::from_bytes(&bytes)
                                                               .unwrap(),
                                                           btclient.get_id(),
                                                           6882)?;
                                    peer_connections(result.0, &result.1, btclient.get_id())?;
                                    btclient.add(torrent_name.to_string(), torrent);
                                }
                                Err(e) => error!("{:?}", e),
                            }
                        }
                    }
                    "remove" | "r" => {
                        if line.len() != 2 {
                            error!("usage: remove <torrent number>");
                        } else {
                            let id = line[1].parse::<u32>().unwrap();
                            btclient.remove(id);
                        }
                    }
                    "list" | "l" => {
                        if line.len() != 1 {
                            error!("usage: list");
                        } else {
                            let t_list = btclient.list().unwrap();
                            println!("ID  Torrent");
                            println!("--  ----------------------------------------------");
                            for t in t_list {
                                println!("{:2}  {}", t.0, t.1);
                            }
                        }
                    }
                    "showfiles" | "sf" => {
                        if line.len() != 2 {
                            error!("usage: showfiles <torrent file>");
                        } else {
                            let path = line[1];
                            match File::open(path) {
                                Ok(mut f) => {
                                    let mut bytes: Vec<u8> = Vec::new();
                                    f.read_to_end(&mut bytes).unwrap();
                                    print_files(&bytes)?;
                                }
                                Err(e) => error!("{:?}", e),
                            }
                        }
                    }
                    "" => {}
                    _ => {
                        println!("invalid command, see \"help\"");
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                info!("CTRL-C");
                break;
            }
            Err(ReadlineError::Eof) => {
                info!("CTRL-D");
                break;
            }
            Err(err) => {
                error!("Error: {:?}", err);
                break;
            }
        }
    }
    rl.save_history(HISTORY_FILE).unwrap();
    Ok(())
}
