use bip_metainfo::MetainfoFile;
use bip_bencode::Bencode;
use bip_utracker::contact::CompactPeersV4;
use errors::*;
use futures::sync::mpsc::{self, Sender, Receiver};
use hyper::Client;
use hyper::header::Connection;
use tokio_core::io::{read, write_all};
use tokio_core::net::TcpStream;
use tokio_core::reactor::Core;
use url::Url;

use std::collections::HashMap;
use std::io::Read;
use std::fs;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use std::thread;
use std::str;
use std::string::String;

use packet::peer_pkt::{MutablePeerHandshakePacket, MutablePeerMessagePacket};

pub struct BTClient {
    torrents: HashMap<usize, Arc<RwLock<Torrent>>>,
    id: String, // peer_id or client id
    next_id: usize,
    channels: HashMap<usize, Sender<Message>>,
}

fn torrent_loop(rx: Receiver<Message>, torrent: Arc<RwLock<Torrent>>) {
    let mut core = Core::new().unwrap();
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

    pub fn add(self: &mut BTClient, file: fs::File) -> Result<()> {
        let (tx, rx): (Sender<Message>, Receiver<Message>) = mpsc::channel(1);
        let torrent = Arc::new(RwLock::new(Torrent::new(file)));
        let t_clone = torrent.clone();
        self.torrents.insert(self.next_id, torrent);
        self.channels.insert(self.next_id, tx);
        self.next_id += 1;

        thread::spawn(move || torrent_loop(rx, t_clone));
        Ok(())
    }

    pub fn remove(self: &mut BTClient, id: usize) -> Result<usize> {
        self.torrents.remove(&id);
        Ok(self.torrents.len())
    }

    pub fn list(self: &BTClient) -> Vec<(usize, String)> {
        self.torrents
            .iter()
            .map(|(id, torrent)| {
                let torrent = &(*(torrent.read().unwrap()));
                let root_name: String;
                if let Some(dir) = torrent.metainfo
                    .info()
                    .directory() {
                    root_name = dir.to_owned();
                } else {
                    root_name = torrent.metainfo
                        .info()
                        .files()
                        .next()
                        .unwrap()
                        .paths()
                        .next()
                        .unwrap()
                        .to_owned();
                }
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
    pub bitmap: Vec<u8>,
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
        let pieces = metainfo.info().pieces().count();
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
            bitmap: vec![0; pieces],
        }
    }
    fn send_bitmap(self: Torrent, mut stream: &TcpStream) -> Result<()> {
        let mut buf = vec![0u8; self.bitmap.len()+2];
        let mut bitmap_pkt = MutablePeerMessagePacket::new(&mut buf).unwrap();
        bitmap_pkt.set_len((self.bitmap.len() + 1) as u32);
        bitmap_pkt.set_id(5);
        bitmap_pkt.set_payload(&self.bitmap);
        Ok(())
    }

    fn connect_to_tracker(self: Torrent,
                          peer_id: String,
                          port: u16)
                          -> Result<(Vec<String>, String)> {
        debug!("connecting to tracker: {:?}", self.metainfo.main_tracker());

        let info_hash = self.metainfo.info_hash();
        // TODO figure can this conversion to url-encoded form be done safely?
        let info_hash_str = unsafe { str::from_utf8_unchecked(info_hash.as_ref()) };

        // TODO this needs to be calculated based on what we have
        let total_len = self.metainfo.info().files().fold(0, |acc, nex| acc + nex.length());

        let mut url = Url::parse(self.metainfo.main_tracker().unwrap()).unwrap();
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
