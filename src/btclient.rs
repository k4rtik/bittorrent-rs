use bip_metainfo::MetainfoFile;
use errors::*;

use std::collections::HashMap;
use std::io::Read;
use std::fs;
use std::sync::mpsc::{self, Sender, Receiver};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct BTClient {
    torrents: HashMap<usize, Torrent>,
    id: String, // peer_id or client id
    next_id: usize,
    channels: HashMap<usize, Sender<Message>>,
}

impl BTClient {
    pub fn new() -> BTClient {
        let now = SystemTime::now();
        let duration = now.duration_since(UNIX_EPOCH).unwrap();
        let client_id = "-bittorrent-rs-".to_owned() + &format!("{}", duration.as_secs() % 100_000);
        BTClient {
            torrents: HashMap::new(),
            id: client_id,
            next_id: 0,
            channels: HashMap::new(),
        }
    }

    pub fn add(self: &mut BTClient, file: fs::File) -> Result<usize> {
        let (tx, rx): (Sender<Message>, Receiver<Message>) = mpsc::channel();
        let torrent = Torrent::new(file);
        self.torrents.insert(self.next_id, torrent);
        self.channels.insert(self.next_id, tx);
        self.next_id += 1;
        Ok(self.next_id)
    }

    pub fn remove(self: &mut BTClient, id: usize) -> Result<usize> {
        self.torrents.remove(&id);
        Ok(self.torrents.len())
    }

    pub fn list(self: &BTClient) -> Vec<(usize, String)> {
        self.torrents
            .iter()
            .map(|(id, torrent)| {
                let root_name = torrent.metainfo
                    .info()
                    .files()
                    .next()
                    .unwrap()
                    .paths()
                    .next()
                    .unwrap()
                    .to_owned();
                (*id, root_name)
            })
            .collect()
    }

    pub fn get_id(self: &BTClient) -> String {
        self.id.clone()
    }
}

pub struct Torrent {
    metainfo: MetainfoFile,

    // From/For tracker
    pub peers: Vec<Peer>,
    pub uploaded: usize,
    pub downloaded: usize,
    pub left: usize,
    pub interval: usize, // in seconds
    pub tracker_id: String,
    pub num_seeders: usize,
    pub num_leachers: usize,
}

pub enum Message {
    StartDownload,
    StartSeed,
    StopDownload,
    StopSeed,
    Exit,
}

impl Torrent {
    pub fn new(mut tfile: fs::File) -> Torrent {
        // parse metainfo file
        let mut bytes: Vec<u8> = Vec::new();
        tfile.read_to_end(&mut bytes).unwrap();

        let metainfo = MetainfoFile::from_bytes(bytes).unwrap();

        Torrent {
            metainfo: metainfo,

            peers: Vec::new(),
            uploaded: 0,
            downloaded: 0,
            left: 0,
            interval: 0, // in seconds
            tracker_id: String::new(),
            num_seeders: 0,
            num_leachers: 0,
        }
    }
}

pub struct FileT {
    pub path: String, // can be file or directory name
    pub length: usize,
    pub md5: Option<String>, // optional, try not to use
}

pub struct Peer {
    id: String, // peer_id
    ip_port: String,

    am_choking: bool,
    peer_choking: bool,
    am_interested: bool,
    peer_interested: bool,
}

impl Peer {
    pub fn new(id: String, ip_port: String) -> Peer {
        Peer {
            id: id,
            ip_port: ip_port,
            am_choking: true,
            am_interested: false,
            peer_choking: true,
            peer_interested: false,
        }
    }
}

struct TrackerRequest {
    info_hash: String,
    peer_id: String,
    port: usize, // port client is listening on, between 6881-6889
    uploaded: usize,
    downloaded: usize,
    left: usize,
    event: EventType, // TODO think about adding optional fields
}

enum EventType {
    Started,
    Stopped,
    Completed,
}
