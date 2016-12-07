use url::Url;
use bip_util::sha::ShaHash;

struct BTClient {
    torrents: Vec<Torrent>,
    id: String, // peer_id or client id
}

struct Torrent {
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
