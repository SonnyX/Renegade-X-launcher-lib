use std::time::{Duration, Instant};
use crate::downloader::download_file;
use crate::traits::{AsString,Error};
use std::sync::{Arc, Mutex};
use std::net::ToSocketAddrs;

#[derive(Debug, Clone)]
pub struct Mirror {
  pub address: Arc<String>,
  pub speed: f64,
  pub ping: f64,
  pub enabled: Arc<Mutex<bool>>,
  pub ip: SocketAddrs,//Vec<std::net::SocketAddr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SocketAddrs {
  inner: Vec<std::net::SocketAddr>
}

impl From<Vec<std::net::SocketAddr>> for SocketAddrs {
  fn from(other: Vec<std::net::SocketAddr>) -> Self {
    SocketAddrs {
      inner: other
    }
  }
}

impl ToSocketAddrs for SocketAddrs {
  type Iter = std::vec::IntoIter<std::net::SocketAddr>;
  fn to_socket_addrs(&self) -> std::io::Result<std::vec::IntoIter<std::net::SocketAddr>> {
    Ok(self.inner.clone().into_iter())
  }
}

#[derive(Clone)]
pub struct LauncherInfo {
  pub version_name: String,
  pub version_number: usize,
  pub patch_url: String,
  pub patch_hash: String,
  pub prompted: bool,
}

pub struct Mirrors {
  pub mirrors: Vec<Mirror>,
  pub instructions_hash: Option<String>,
  pub version_number: Option<String>,
  pub launcher_info: Option<LauncherInfo>,
}

impl Mirrors {
  pub fn new() -> Mirrors {
    Mirrors {
      mirrors: Vec::new(),
      instructions_hash: None,
      version_number: None,
      launcher_info: None,
    }
  }

  pub fn is_empty(&self) -> bool {
    self.mirrors.is_empty()
  }

  pub fn remove(&self, entry: Mirror) {
    for i in 0..self.mirrors.len() {
      if self.mirrors[i].ip == entry.ip {
        self.disable(i);
      }
    }
  }

  pub fn disable(&self, entry: usize) {
    let mirrors = self.mirrors[entry].enabled.clone();
    *mirrors.lock().expect(concat!(module_path!(),":",file!(),":",line!())) = false;
  }

  /**
  Downloads release.json from the renegade-x server and adds it to the struct
  */
  pub fn get_mirrors(&mut self, location: &str) -> Result<(), Error> {
    let mut release_json = match download_file(location.to_string(), Duration::from_secs(10)) {
      Ok(result) => result,
      Err(e) => return Err(format!("Is your internet down? {}", e).into())
    };
    let release_json_response = match release_json.text() {
      Ok(result) => result,
      Err(e) => return Err(format!("mirrors.rs: Corrupted response: {}", e).into())
    };
    let release_data = match json::parse(&release_json_response) {
      Ok(result) => result,
      Err(e) => return Err(format!("mirrors.rs: Invalid JSON: {}", e).into())
    };
    self.launcher_info = Some(LauncherInfo {
      version_name: release_data["launcher"]["version_name"].as_string(),
      version_number: release_data["launcher"]["version_number"].as_usize().expect(concat!(module_path!(),":",file!(),":",line!())),
      patch_url: release_data["launcher"]["patch_url"].as_string(),
      patch_hash: release_data["launcher"]["patch_hash"].as_string(),
      prompted: false,
    });
    let mut mirror_vec = Vec::with_capacity(release_data["game"]["mirrors"].len());
    release_data["game"]["mirrors"].members().for_each(|mirror| mirror_vec.push(mirror["url"].as_string()) );
    for mirror in mirror_vec {
      let url = mirror.parse::<url::Url>().expect(concat!(module_path!(),":",file!(),":",line!()));
      self.mirrors.push(Mirror{
        address: Arc::new(format!("{}{}", &mirror, release_data["game"]["patch_path"].as_string())),
        ip: url.socket_addrs(|| None).expect(concat!(module_path!(),":",file!(),":",line!())).into(),
        speed: 1.0,
        ping: 1000.0,
        enabled: Arc::new(Mutex::new(false)),
      });
    }
    self.test_mirrors()?;
    println!("{:#?}", &self.mirrors);
    self.instructions_hash = Some(release_data["game"]["instructions_hash"].as_string());
    self.version_number = Some(release_data["game"]["version_number"].as_u64().expect(concat!(module_path!(),":",file!(),":",line!())).to_string());
    Ok(())
  }

  
  pub fn get_mirror(&self) -> Mirror {
    for i in 0..20 {
      for mirror in self.mirrors.iter() {
        if *mirror.enabled.lock().expect(concat!(module_path!(),":",file!(),":",line!())) && Arc::strong_count(&mirror.address) == i {
          println!("i: {}, mirror: {}", i, &mirror.address);
          return mirror.clone();
        }
      }
    }
    panic!("No mirrors found?");
  }

  /**
  Checks the speed on the mirrors again
  */
  pub fn test_mirrors(&mut self) -> Result<(), Error> {
    let mut handles = Vec::new();
    for i in 0..self.mirrors.len() {
      let mirror = self.mirrors[i].clone();
      let fastest_mirror_speed = self.mirrors[0].speed;
      handles.push(std::thread::spawn(move || -> Mirror {
        let start = Instant::now();
        let mut url = format!("{}", mirror.address.to_owned());
        url.truncate(url.rfind('/').expect(concat!(module_path!(),":",file!(),":",line!())) + 1);
        url.push_str("10kb_file");
        let download_response = download_file(url, Duration::from_millis(10_000/fastest_mirror_speed as u64 * 4));
        match download_response {
          Ok(result) => {
            let duration = start.elapsed();
            let content_length = result.headers().get("content-length");
            if content_length.is_none() || content_length.expect(concat!(module_path!(),":",file!(),":",line!())) != "10000" {
              Mirror { 
                address: mirror.address,
                ip: mirror.ip,
                speed: 0.0,
                ping: 1000.0,
                enabled: Arc::new(Mutex::new(false)),
              }
            } else {
              Mirror { 
                address: mirror.address,
                ip: mirror.ip,
                speed: 10_000.0/(duration.as_millis() as f64),
                ping: (duration.as_micros() as f64)/1000.0,
                enabled: Arc::new(Mutex::new(true)),
              }
            }
          },
          Err(_e) => {
            Mirror { 
              address: mirror.address,
              ip: mirror.ip,
              speed: 0.0,
              ping: 1000.0,
              enabled: Arc::new(Mutex::new(false)),
            }
          }
        }
      }));
    }
    for handle in handles {
      let mirror = handle.join().expect("Failed to execute thread in test_mirrors!");
      for i in 0..self.mirrors.len() {
        if self.mirrors[i].address == mirror.address {
          self.mirrors[i] = mirror;
          break;
        }
      }
    }
    if self.mirrors.len() > 1 {
      self.mirrors.sort_by(|a,b| b.speed.partial_cmp(&a.speed).expect(concat!(module_path!(),":",file!(),":",line!())));
      let best_speed = self.mirrors[0].speed;
      for mut elem in self.mirrors.iter_mut() {
        if elem.speed < best_speed / 4.0 {
          elem.enabled = Arc::new(Mutex::new(false));
        }
      }
    }
    Ok(())
  }
}
