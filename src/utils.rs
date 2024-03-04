use image::imageops::FilterType;
use image::GenericImageView;
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub image_folder: String,
    pub pwd: String,
    
}

pub fn read_config(config_file: &str) -> Config {
    match File::open(config_file) {
        Ok(file) => {
            let reader = BufReader::new(file);
            serde_json::from_reader(reader).unwrap()
        }
        Err(_) => {
            // Dump the default config to the file
            let default_config = Config {
                host: "127.0.0.1".to_string(),
                port: 8080,
                image_folder: "./images".to_string(),
                pwd: "secret".to_string(),
            };
            let serialized = serde_json::to_string_pretty(&default_config).unwrap();
            let mut file = File::create(config_file).expect("Unable to create config file");
            // Create the folder if it doesn't exist
            fs::create_dir_all("./images/pc").expect("Unable to create image folder for pc");
            fs::create_dir_all("./images/mp").expect("Unable to create image folder for mp");
            fs::create_dir_all("./images/thumbnails").expect("Unable to create thumbnails folder");
            file.write_all(serialized.as_bytes())
                .expect("Unable to write to config file");
            println!("Default config created: {}", config_file);
            default_config
        }
    }
}

// Create thumbnails
pub fn create_thumbnail(
    image_path: &Path,
    max_width: u32,
    max_height: u32,
    image_folder: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let thumbnails_dir = PathBuf::from(image_folder).join("thumbnails");
    // Read the image
    let img = image::open(image_path)?;

    // Construct the path for the thumbnail
    let thumbnail_path = thumbnails_dir.join(image_path.file_name().unwrap());

    // Check if the thumbnail already exists
    if thumbnail_path.exists() {
        return Ok(());
    }

    // Calculate the thumbnail dimensions while preserving the aspect ratio
    let (orig_width, orig_height) = img.dimensions();
    let ratio = f64::from(orig_width) / f64::from(orig_height);
    let (new_width, new_height) = if ratio > 1.0 {
        // width greater than height
        let height = f64::from(max_width) / ratio;
        (max_width, height as u32)
    } else {
        // height greater than width
        let width = f64::from(max_height) * ratio;
        (width as u32, max_height)
    };

    // Resize the image
    let thumbnail = img.resize(new_width, new_height, FilterType::Lanczos3);

    // Save the thumbnail to the file
    thumbnail.save(thumbnail_path)?;

    Ok(())
}

// Recursively create thumbnails
pub fn create_thumbnails(
    folder_path: &str,
    max_width: u32,
    max_height: u32,
    image_folder: &str,
) -> std::io::Result<usize> {
    // Return if the folder is 'thumbnails'
    if folder_path.ends_with("thumbnails") {
        return Ok(0);
    }
    let mut thumbnail_count = 0;
    // Recursively iterate through the folder
    for entry in fs::read_dir(folder_path)? {
        let entry = entry?;
        let path = entry.path();

        // Check if the path is a file
        if path.is_file() {
            // Filter the files by extension
            if let Some(ext) = path.extension() {
                if ext == "webp" {
                    // Check if the thumbnail already exists
                    let current_folder = Path::new(folder_path)
                        .file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_string();
                    let thumbnail_path = path
                        .to_str()
                        .unwrap()
                        .replace(&current_folder, "thumbnails");
                    if Path::new(&thumbnail_path).exists() {
                        continue;
                    }

                    // Try to create a thumbnail
                    match create_thumbnail(&path, max_width, max_height, image_folder) {
                        Ok(_) => {
                            println!("Thumbnail created for {:?}", path);
                            thumbnail_count += 1;
                        }
                        Err(e) => eprintln!("Failed to create thumbnail for {:?}: {}", path, e),
                    }
                }
            }
        } else if path.is_dir() {
            // If the path is a directory, call the function recursively
            thumbnail_count +=
                create_thumbnails(&path.to_str().unwrap(), max_width, max_height, image_folder)?;
        }
    }
    Ok(thumbnail_count)
}

// Convert the image to webp format
pub fn convert_images_to_webp(folder_path: &str) -> std::io::Result<usize> {
    let mut converted_count = 0;
    // Recursively iterate through the folder
    for entry in fs::read_dir(folder_path)? {
        let entry = entry?;
        let path = entry.path();

        // Check if the path is a file
        if path.is_file() {
            // Filter the files by extension
            if let Some(ext) = path.extension() {
                if ext == "jpg" || ext == "png" || ext == "jpeg" {
                    // Try to open the image
                    match image::open(&path) {
                        Ok(img) => {
                            // Create a new path with the same name but with the webp extension
                            let new_path = path.with_extension("webp");

                            // Save the image in webp format
                            match img.save_with_format(new_path, image::ImageFormat::WebP) {
                                Ok(_) => {
                                    // Remove the original image
                                    fs::remove_file(&path)?;
                                    println!("Converted {:?} to webp.", path);
                                    converted_count += 1;
                                }
                                Err(e) => eprintln!("Failed to convert {:?}: {}", path, e),
                            }
                        }
                        Err(e) => eprintln!("Failed to open {:?}: {}", path, e),
                    }
                }
            }
        } else if path.is_dir() {
            // If the path is a directory, call the function recursively
            converted_count += convert_images_to_webp(&path.to_str().unwrap())?;
        }
    }
    Ok(converted_count)
}

pub fn validate_folder(folder: &str) -> std::io::Result<()> {
    // Check if the folder exists
    if !Path::new(folder).exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Image folder not found.",
        ));
    }
    // Validate the structure of the folder, the folder should contain subfolders 'pc' and 'mp'
    if !Path::new(&format!("{}/pc", folder)).exists()
        || !Path::new(&format!("{}/mp", folder)).exists()
        || !Path::new(&format!("{}/thumbnails", folder)).exists()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Invalid image folder structure.",
        ));
    }
    Ok(())
}

pub fn create_folder_structure(folder: &str) -> std::io::Result<()> {
    // Create the folder if it doesn't exist
    fs::create_dir_all(&folder)?;

    // Create the subfolders
    fs::create_dir_all(&format!("{}/pc", folder))?;
    fs::create_dir_all(&format!("{}/mp", folder))?;
    fs::create_dir_all(&format!("{}/thumbnails", folder))?;

    Ok(())
}

pub fn index_images(folder: &str) -> Vec<Vec<String>> {
    // WalkDir::new(folder)
    //     .into_iter()
    //     .filter_map(|e| e.ok())
    //     .filter(|e| {
    //         e.path()
    //             .extension()
    //             .and_then(std::ffi::OsStr::to_str)
    //             .unwrap_or("")
    //             == "webp"
    //     })
    //     .map(|e| e.path().to_str().unwrap().to_string())
    //     .collect()

    // Create a vector to store the image paths excluding the thumbnails
    let mut images = Vec::new();
    let mut pc_images = Vec::new();
    let mut mp_images = Vec::new();
    // Iterate through the folder
    for entry in WalkDir::new(folder) {
        let entry = entry.unwrap();
        let path = entry.path();
        // Check if the path is a file
        if path.is_file() {
            // Filter the files by extension
            if let Some(ext) = path.extension() {
                if ext == "webp" {
                    let subfolder = path
                        .parent()
                        .unwrap()
                        .file_name()
                        .unwrap()
                        .to_str()
                        .unwrap();
                    match subfolder {
                        "pc" => pc_images.push(path.to_str().unwrap().to_string()),
                        "mp" => mp_images.push(path.to_str().unwrap().to_string()),
                        _ => (),
                    }
                }
            }
        }
    }
    images.push(pc_images);
    images.push(mp_images);
    images
}
