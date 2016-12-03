extern crate bip_metainfo;
extern crate chrono;
#[macro_use]
extern crate log;
extern crate pretty_env_logger;
extern crate rustyline;

use bip_metainfo::MetainfoFile;
use chrono::{TimeZone, UTC};
use rustyline::Editor;
use rustyline::error::ReadlineError;

use std::io::prelude::*;
use std::fs::File;

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

fn connect(tracker: &str) {
    debug!("connecting to tracker: {:?}", tracker);

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
                                    connect(MetainfoFile::from_bytes(&bytes)
                                        .unwrap()
                                        .main_tracker()
                                        .unwrap());
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
