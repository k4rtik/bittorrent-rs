#[macro_use]
extern crate log;
#[macro_use]
extern crate nom;

extern crate bip_bencode;
extern crate bip_metainfo;
extern crate bip_utracker;
extern crate bip_util;
extern crate chrono;
extern crate hyper;
extern crate pnet;
extern crate pnet_macros_support;
extern crate pretty_env_logger;
extern crate rustyline;
extern crate url;

mod packet;

use bip_bencode::Bencode;
use bip_metainfo::MetainfoFile;
use bip_utracker::contact::CompactPeersV4;
use chrono::{TimeZone, UTC};
use hyper::Client;
use hyper::header::Connection;
use packet::peer_pkt::{MutablePeerHandshakePacket, MutablePeerMessagePacket};
use pnet::packet::Packet;
use rustyline::Editor;
use rustyline::error::ReadlineError;
use url::Url;

use std::fs::File;
use std::io::prelude::*;
use std::net::TcpStream;
use std::str;
use std::string::String;
use std::time::{SystemTime, UNIX_EPOCH};
use std::thread;

const HISTORY_FILE: &'static str = ".rustyline.history";
const PEER_HANDSHAKE_STRUCT_SZ: usize = 68;
const PEER_REQ_PKT_SZ: usize = 14;

/// Print general information about the torrent.
fn print_metainfo_overview(bytes: &[u8]) {
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
    println!("Total File Size: {}",
             info.files().fold(0, |acc, nex| acc + nex.length()));
}

fn connect_to_tracker(metainfo: MetainfoFile,
                      peer_id: &str,
                      port: u16)
                      -> Result<(Vec<String>, String), String> {
    debug!("connecting to tracker: {:?}", metainfo.main_tracker());

    let info_hash = metainfo.info_hash();
    // TODO figure can this conversion to url-encoded form be done safely?
    let info_hash_str = unsafe { str::from_utf8_unchecked(info_hash.as_ref()) };

    // TODO this needs to be calculated based on what we have
    let total_len = metainfo.info().files().fold(0, |acc, nex| acc + nex.length());

    let mut url = Url::parse(metainfo.main_tracker().unwrap()).unwrap();
    url.query_pairs_mut()
        .append_pair("info_hash", info_hash_str)
        .append_pair("peer_id", peer_id)
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

fn peer_connections(peer_ip_ports: Vec<String>,
                    info_hash: &str,
                    peer_id: &str)
                    -> Result<(), String> {
    for peer_ip_port in peer_ip_ports {
        let peer_ip_port_cl = peer_ip_port.clone();
        let info_hash_cl = info_hash.to_string().clone();
        let peer_id_cl = peer_id.to_string().clone();
        thread::spawn(move || {
            match handshake_peer(&peer_ip_port_cl, &info_hash_cl, &peer_id_cl) {
                Ok(mut stream) => {
                    debug!("Communicating with peer {:?}", peer_ip_port_cl);
                    let mut buff = [0; PEER_HANDSHAKE_STRUCT_SZ];
                    match stream.read(&mut buff) {
			Ok(_) => {
				let mut buf = vec![0u8; PEER_REQ_PKT_SZ];
				let mut peer_msg_pkt  = MutablePeerMessagePacket::new(&mut buf).unwrap();
				peer_msg_pkt.set_len(13);
				peer_msg_pkt.set_id(6);
				//TODO fill length properly
				let index_begin_length = vec![0, 0, 10];
				peer_msg_pkt.set_payload(&index_begin_length);
				match stream.write(peer_msg_pkt.packet()) {
					Ok(_) => {
						let mut buf_read = vec![0; 2048];
						match stream.read(&mut buf_read) {
							Ok(bytes_read) => {debug!("Bytes read: {:?}", bytes_read);},
							Err(e) => error!("Read failed! {:?}", e),
						}
					},
					Err(e) => error!("Write to stream failed! {:?}", e),
				}
			},
			Err(e) => error!("Reading from the stream failed! {:?}", e),
			}
                }
                Err(_) => {
                    error!("Closing thread with peer {:?}", peer_ip_port_cl);
                }
            }
        });
    }
    Ok(())
}

fn handshake_peer(peer_ip_port: &str, info_hash: &str, peer_id: &str) -> Result<TcpStream, String> {
    let mut buf = vec![0u8; PEER_HANDSHAKE_STRUCT_SZ];
    let mut ph = MutablePeerHandshakePacket::new(&mut buf).unwrap();
    ph.set_pstrlen("BitTorrent protocol".len() as u8);
    ph.set_pstr(&String::from("BitTorrent protocol").into_bytes());
    ph.set_reserved(&[0; 8]);
    ph.set_info_hash(info_hash.as_bytes());
    ph.set_peer_id(peer_id.as_bytes());
    match TcpStream::connect(peer_ip_port) {
        Ok(mut stream) => {
            debug!("Connection to peer {:?} successful!", peer_ip_port);
            match stream.write(ph.packet()) {
                Ok(_) => {
                    debug!("Sending message successful!");
                    trace!("{:?}", ph.packet());
                    Ok((stream))
                }
                Err(_) => {
                    error!("Sending message failed!");
                    Err("Sending message failed!".to_owned())
                }
            }
        }
        Err(e) => {
            error!("Connection to peer {:?} failed! {:?}", peer_ip_port, e);
            Err("Connection to peer failed!".to_owned())
        }
    }
}

fn main() {
    pretty_env_logger::init();

    let mut rl = Editor::<()>::new();
    if rl.load_history(HISTORY_FILE).is_err() {
        info!("No previous history!");
    }

    let now = SystemTime::now();
    let duration = now.duration_since(UNIX_EPOCH).unwrap();
    let client_id = "-bittorrent-rs-".to_owned() + &format!("{}", duration.as_secs() % 100_000);

    loop {
        let readline = rl.readline("> ");
        match readline {
            Ok(cmd) => {
                rl.add_history_entry(&cmd);

                let cmd = cmd.trim().split(' ').collect::<Vec<&str>>();

                match cmd[0] {
                    "parse" => {
                        if cmd.len() != 2 {
                            error!("usage: parse <torrent file>");
                        } else {
                            let path = cmd[1];
                            debug!("MetainfoFile: {:?}", path);

                            match File::open(path) {
                                Ok(mut f) => {
                                    let mut bytes: Vec<u8> = Vec::new();
                                    f.read_to_end(&mut bytes).unwrap();
                                    print_metainfo_overview(&bytes);
                                }
                                Err(e) => error!("{:?}", e),
                            }
                        }
                    }
                    "connect" => {
                        if cmd.len() != 2 {
                            error!("usage: connect <torrent file>");
                        } else {
                            let path = cmd[1];
                            match File::open(path) {
                                Ok(mut f) => {
                                    let mut bytes: Vec<u8> = Vec::new();
                                    f.read_to_end(&mut bytes).unwrap();

                                    // TODO: generate peer ID
                                    let result =
                                        connect_to_tracker(MetainfoFile::from_bytes(&bytes)
                                                               .unwrap(),
                                                           &client_id,
                                                           6882)
                                            .unwrap();
                                    peer_connections(result.0, &result.1, &client_id).unwrap();
                                }
                                Err(e) => error!("{:?}", e),
                            }
                        }
                    }
                    _ => {}
                }
            }
            Err(ReadlineError::Interrupted) |
            Err(ReadlineError::Eof) => {
                break;
            }
            Err(err) => {
                error!("Error: {:?}", err);
                break;
            }
        }
    }
    rl.save_history(HISTORY_FILE).unwrap();
}
