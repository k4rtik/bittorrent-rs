use bip_util::sha::ShaHash;
use errors::*;
use url::Url;
use std::collections::HashMap;

use std::time::{SystemTime, UNIX_EPOCH};

pub struct BTClient {
    torrents: HashMap<u32, Torrent>,
    id: String, // peer_id or client id
    torr_count: u32,
}

impl BTClient {
    pub fn new() -> BTClient {
        let now = SystemTime::now();
        let duration = now.duration_since(UNIX_EPOCH).unwrap();
        let client_id = "-bittorrent-rs-".to_owned() + &format!("{}", duration.as_secs() % 100_000);
        BTClient {
            torrents: HashMap::new(),
            id: client_id,
            torr_count: 0,
        }
    }

    pub fn add(self: &mut BTClient, torrent_name: String, mut torrent: Torrent) -> Result<usize> {
        self.torr_count += 1;
        torrent.name = torrent_name;
        self.torrents.insert(self.torr_count, torrent);
        Ok(self.torrents.len())
    }
    pub fn remove(self: &mut BTClient, id: u32) -> Result<usize> {
        self.torrents.remove(&id);
        Ok(self.torrents.len())
    }
    pub fn list(self: &BTClient) -> Result<Vec<(u32, String)>> {
        let mut t_list = Vec::new();
        for t in &self.torrents {
            let res = (t.0.clone(), t.1.name.clone());
            t_list.push(res);
        }
        Ok(t_list)
    }
    pub fn get_id(self: &BTClient) -> String {
        self.id.clone()
    }
}

pub struct Torrent {
    // MetaInfo
    announce: Url,
    piece_length: usize,
    pieces_sha1: Vec<ShaHash>,
    name: String, // can be file or directory name
    length: Option<usize>,
    md5: Option<String>, // optional, try not to use
    files: Option<File>,

    // From/For tracker
    peers: Vec<Peer>,
    uploaded: usize,
    downloaded: usize,
    left: usize,
    interval: usize, // in seconds
    tracker_id: String,
    num_seeders: usize,
    num_leachers: usize,
}

impl Torrent {
    pub fn new(url: Url, plen: usize, pieces: Vec<ShaHash>) -> Torrent {
        Torrent {
            announce: url,
            piece_length: plen,
            pieces_sha1: pieces,
            name: String::new(),
            length: None,
            md5: None,
            files: None,

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

struct File {
    path: String, // can be file or directory name
    length: usize,
    md5: Option<String>, // optional, try not to use
}

struct Peer {
    id: String, // peer_id
    ip_port: String,

    am_choking: bool,
    peer_choking: bool,
    am_interested: bool,
    peer_interested: bool,
}

impl Peer {
    fn new(id: String, ip_port: String) -> Peer {
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
