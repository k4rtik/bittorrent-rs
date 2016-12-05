#[macro_use]
extern crate log;
extern crate bip_bencode;
extern crate bip_metainfo;
extern crate chrono;
extern crate hyper;
extern crate pretty_env_logger;
extern crate rustyline;
extern crate url;

use bip_bencode::Bencode;
use bip_metainfo::MetainfoFile;
use chrono::{TimeZone, UTC};
use hyper::Client;
use hyper::header::Connection;
use rustyline::Editor;
use rustyline::error::ReadlineError;
use url::Url;

use std::fs::File;
use std::io::prelude::*;
use std::str;
use std::string::String;

const HISTORY_FILE: &'static str = ".rustyline.history";

/// Print general information about the torrent.
fn print_metainfo_overview(bytes: &[u8]) {
    let metainfo = MetainfoFile::from_bytes(bytes).unwrap();
    let info = metainfo.info();
    let info_hash_hex = metainfo.info_hash()
        .as_ref()
        .iter()
        .map(|b| format!("{:02X}", b))
        .fold(String::new(), |mut acc, nex| {
            acc.push_str(&nex);
            acc
        });
    let utc_creation_date = metainfo.creation_date().map(|c| UTC.timestamp(c, 0));

    println!("\n\n-----------------------------Metainfo File Overview-----------------------------");

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

fn connect_to_tracker(metainfo_file: MetainfoFile, peer_id: &str, port: u16) -> Result<(), String> {
    debug!("connecting to tracker: {:?}", metainfo_file.main_tracker());
    let client = Client::new();
    let mut url = Url::parse(metainfo_file.main_tracker().unwrap()).unwrap();
    let info_hash = metainfo_file.info_hash();
    let info_hash_str = unsafe { str::from_utf8_unchecked(info_hash.as_ref()) };
    url.query_pairs_mut().append_pair("info_hash", &info_hash_str);
    url.query_pairs_mut().append_pair("peer_id", "-TR2920-utffmgat89lc");
    url.query_pairs_mut().append_pair("port", &(port.to_string()));
    url.query_pairs_mut().append_pair("uploaded", "0");
    url.query_pairs_mut().append_pair("downloaded", "0");
    let total_len = metainfo_file.info().files().fold(0, |acc, nex| acc + nex.length());
    url.query_pairs_mut().append_pair("left", &(total_len.to_string()));
    url.query_pairs_mut().append_pair("compact", "1");
    url.query_pairs_mut().append_pair("event", "started");
    debug!("URL {:?}", url);
    let mut res = client.get(url).header(Connection::close()).send().unwrap();
    let mut buffer = Vec::new();
    res.read(&mut buffer);
    debug!("Resp {:?}", res);
    let bencode = Bencode::decode(&buffer).unwrap();
    debug!("{:?}", bencode);

    Ok(())
}

fn main() {
    pretty_env_logger::init();

    let mut rl = Editor::<()>::new();
    if rl.load_history(HISTORY_FILE).is_err() {
        info!("No previous history!");
    }
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
                                    connect_to_tracker(MetainfoFile::from_bytes(&bytes).unwrap(),
                                                       "myid",
                                                       6882 /* .unwrap()
                                                             * .main_tracker() */);
                                }
                                Err(e) => error!("{:?}", e),
                            }
                        }
                    }
                    _ => {}
                }
            }
            Err(ReadlineError::Interrupted) => {
                break;
            }
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
