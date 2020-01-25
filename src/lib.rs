use std::{
    error::Error,
    env,
    fs,
    fs::{File, OpenOptions},
    path::{Path, PathBuf, Component},
    io,
    io::{SeekFrom, prelude::*}
};
use glob::glob;
use serde_json::{json, Value};

const MAX_SIZE: u64 = 4294967295;

fn align_size(size: usize) -> usize {
    size + (4 - (size % 4)) % 4
}

fn read_u32(buffer: &[u8]) -> u32 {
	let mut result = 0;
	for i in 0..4 {
		result += (buffer[i] as u32) << i * 8;
	}
	result
}

fn write_u32(buffer: &mut [u8], value: u32) {
    for i in 0..4 {
        buffer[i] = (value >> i * 8) as u8;
    }
}

fn read_header(reader: &mut File) -> Result<(u32, Value), Box<dyn Error>> {
    // read header bytes
    let mut header_buffer = vec![0u8; 16];
    reader.read_exact(&mut header_buffer)?;

    // grab sizes
    let header_size = read_u32(&header_buffer[4..8]);
    let json_size = read_u32(&header_buffer[12..]);

    // read json bytes
    let mut json_buffer = vec![0u8; json_size as usize];
    reader.read_exact(&mut json_buffer)?;

    // parse json
    let json: Value = serde_json::from_slice(&json_buffer)?;

    Ok((header_size + 8, json))
}

fn iterate_entries(json: &Value, mut callback: impl FnMut(&Value, &PathBuf)) {
    iterate_entries_err(json, |current, path| {
        callback(current, path);
        Ok(())
    }).expect("Error iterating entries");
}

fn iterate_entries_err(json: &Value, mut callback: impl FnMut(&Value, &PathBuf) -> Result<(), Box<dyn Error>>) -> Result<(), Box<dyn Error>> {
    fn helper(current: &Value, path: PathBuf, callback: &mut impl FnMut(&Value, &PathBuf) -> Result<(), Box<dyn Error>>) -> Result<(), Box<dyn Error>> {
        callback(current, &path)?;
        if current["files"] != Value::Null {
            for (key, val) in current["files"].as_object().unwrap() {
                helper(&val, path.join(key), callback)?;
            }
        }
        Ok(())
    }
    for (key, val) in json["files"].as_object().unwrap() {
        helper(val, PathBuf::new().join(key), &mut callback)?;
    }
    Ok(())
}

pub fn list(archive: &str) -> Result<(), Box<dyn Error>> {
    let mut file = File::open(archive)?;

    // read header
    let (_, json) = read_header(&mut file)?;

    // list files
    iterate_entries(&json, |_, path| println!("\\{}", path.to_str().expect("Error converting OS path to string")));

    Ok(())
}

pub fn pack(path: &str, dest: &str) -> Result<(), Box<dyn Error>> {
    let mut header_json = json!({
        "files": {}
    });
    let mut files = vec![];
    let dir = env::current_dir()?.join(path);
    if dir.exists() {
        fn walk_dir(dir: impl AsRef<Path>, json: &mut Value, mut offset: &mut u64) -> Result<Vec<PathBuf>, Box<dyn Error>> {
            let mut files = vec![];
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let name = entry.file_name().into_string().expect("Error converting OS path to string");
                let meta = entry.metadata()?;
                if meta.is_file() {
                    let size = meta.len();
                    if size > MAX_SIZE {
                        panic!("File {} ({} GB) is above the maximum possible size of {} GB", name, size as f64 / 1e9, MAX_SIZE as f64 / 1e9);
                    }
                    json[&name] = json!({
                        "offset": offset.to_string(),
                        "size": size
                    });
                    *offset += size;
                    files.push(entry.path());
                }
                else {
                    json[&name] = json!({
                        "files": {}
                    });
                    files.append(&mut walk_dir(entry.path(), &mut json[&name]["files"], &mut offset)?);
                }
            }
            Ok(files)
        }
        files = walk_dir(dir, &mut header_json["files"], &mut 0)?;
    }
    else if let Ok(entries) = glob(path) {
        let mut offset = 0u64;
        for entry in entries {
            let entry = entry?;
            let mut current = &mut header_json["files"];
            let comps: Vec<&Path> = entry.components().map(|comp| match comp {
                Component::Normal(name) => Path::new(name),
                _ => unreachable!()
            }).collect();
            for i in 0..comps.len() - 1 {
                let name = comps[i].file_name().unwrap().to_str().expect("Error converting OS path to string");
                current = &mut current[name]["files"];
            }
            let name = entry.file_name().unwrap().to_str().expect("Error converting OS path to string");
            if entry.is_file() {
                let size = entry.metadata()?.len();
                if size > MAX_SIZE {
                    panic!("File {} ({} GB) is above the maximum possible size of {} GB", name, size as f64 / 1e9, MAX_SIZE as f64 / 1e9);
                }
                current[name] = json!({
                    "offset": offset.to_string(),
                    "size": size
                });
                offset += size;
                files.push(entry);
            }
            else {
                current[name] = json!({
                    "files": {}
                });
            }
        }
    }
    else {
        panic!("{} is neither a valid directory nor glob", path);
    }

    // create header buffer wtih json
    let mut header = serde_json::to_vec(&header_json)?;

    
    // compute sizes
    let json_size = header.len();
    let size = align_size(json_size);

    // resize header
    header.resize(16 + size, 0);

    // copy json
    header.copy_within(0..json_size, 16);

    // write sizes into header
    write_u32(&mut header[0..4], 4);
    write_u32(&mut header[4..8], 8 + size as u32);
    write_u32(&mut header[8..12], 4 + size as u32);
    write_u32(&mut header[12..16], json_size as u32);
    
    // write header
    fs::write(dest, &header)?;

    // copy file contents
    let mut archive = OpenOptions::new()
        .write(true)
        .append(true)
        .open(dest)?;
    for filename in files {
        io::copy(&mut File::open(filename)?, &mut archive)?;
    }

	Ok(())
}

pub fn extract(archive: &str, dest: &str) -> Result<(), Box<dyn Error>> {
    let mut file = File::open(archive)?;

    // read header
    let (header_size, json) = read_header(&mut file)?;

    // create destination folder
    let dest = env::current_dir()?.join(dest);
    if !dest.exists() {
        fs::create_dir(&dest)?;
    }

    // iterate over entries
    iterate_entries_err(&json, |val, path| {
        if val["offset"] != Value::Null {
            let offset = val["offset"].as_str().unwrap().parse::<u64>()?;
            let size = val["size"].as_u64().unwrap();
            file.seek(SeekFrom::Start(header_size as u64 + offset))?;
            let mut buffer = vec![0u8; size as usize];
            file.read_exact(&mut buffer)?;
            fs::write(dest.join(path), buffer)?;
        }
        else {
            let dir = dest.join(path);
            if !dir.exists() {
                fs::create_dir(dir)?;
            }
        }
        Ok(())
    })?;

    Ok(())
}

pub fn extract_file(archive: &str, dest: &str) -> Result<(), Box<dyn Error>> {
    let cwd = env::current_dir()?;
    let full_path = cwd.join(dest);
    let dest = cwd.join(Path::new(dest).file_name().unwrap());
    let mut file = File::open(archive)?;

    // read header
    let (header_size, json) = read_header(&mut file)?;

    // iterate over entries
    iterate_entries_err(&json, |val, path| {
        if cwd.join(path) == full_path {
            let offset = val["offset"].as_str().unwrap().parse::<u64>()?;
            let size = val["size"].as_u64().unwrap();
            file.seek(SeekFrom::Start(header_size as u64 + offset))?;
            let mut buffer = vec![0u8; size as usize];
            file.read_exact(&mut buffer)?;
            fs::write(&dest, buffer)?;
        }
        Ok(())
    })?;

    Ok(())
}