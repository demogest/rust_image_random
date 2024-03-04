use actix_multipart::Multipart;
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer, Responder};
use base64::prelude::*;
use futures::{StreamExt, TryStreamExt};
use image::imageops::FilterType;
use image::{io::Reader as ImageReader, GenericImageView, ImageFormat};
use md5::{Digest, Md5};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    host: String,
    port: u16,
    image_folder: String,
    pwd: String,
}

fn read_config(config_file: &str) -> Config {
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
fn create_thumbnail(
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
fn create_thumbnails(
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
fn convert_images_to_webp(folder_path: &str) -> std::io::Result<usize> {
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

fn validate_folder(folder: &str) -> std::io::Result<()> {
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

fn create_folder_structure(folder: &str) -> std::io::Result<()> {
    // Create the folder if it doesn't exist
    fs::create_dir_all(&folder)?;

    // Create the subfolders
    fs::create_dir_all(&format!("{}/pc", folder))?;
    fs::create_dir_all(&format!("{}/mp", folder))?;
    fs::create_dir_all(&format!("{}/thumbnails", folder))?;

    Ok(())
}

fn index_images(folder: &str) -> Vec<String> {
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
    // Iterate through the folder
    for entry in WalkDir::new(folder) {
        let entry = entry.unwrap();
        let path = entry.path();
        // Check if the path is a file
        if path.is_file() {
            // Filter the files by extension
            if let Some(ext) = path.extension() {
                if ext == "webp" {
                    // Check if the path contains 'thumbnails'
                    if !path.to_str().unwrap().contains("thumbnails") {
                        images.push(path.to_str().unwrap().to_string());
                    }
                }
            }
        }
    }

    images
}

// Get the specified thumbnail
#[actix_web::get("/api/thumbnail/{filename}")]
async fn get_thumbnail(
    filename: web::Path<String>,
    data: web::Data<Vec<String>>,
) -> impl Responder {
    let filename = filename.into_inner();
    let img_folder = data[data.len() - 2].clone();
    let mut file = File::open(format!("{}/thumbnails/{}", img_folder, filename)).unwrap();
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).unwrap();
    HttpResponse::Ok().content_type("image/jpeg").body(buffer)
}

// Get the specified image
#[actix_web::get("/api/image/{filename}")]
async fn get_image(
    filename: web::Path<String>,
    data: web::Data<Vec<String>>,
    req: HttpRequest,
) -> impl Responder {
    let filename = filename.into_inner();
    let data = data[0..data.len() - 2].to_vec();
    let file_path = data.iter().find(|&path| path.contains(&filename));
    // Get the visitor's ip address and print to log
    let ip_str = if let Some(cf_ip) = req.headers().get("CF-Connecting-IP") {
        cf_ip.to_str().unwrap_or("").to_string() // Convert to String
    } else if let Some(peer_addr) = req.peer_addr() {
        peer_addr.ip().to_string()
    } else {
        "".to_string() // Could not get the ip address
    };

    // Get the visitor's country and print to log
    let country = if let Some(cf_country) = req.headers().get("CF-IPCountry") {
        cf_country.to_str().unwrap_or("Unknown country").to_string()
    } else {
        "Unknown country".to_string()
    };

    println!(
        "Visitor IP: {}, Country: {}, file: {}",
        ip_str, country, filename
    );
    
    if let Some(file_path) = file_path {
        let mut file = File::open(file_path).unwrap();
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).unwrap();
        HttpResponse::Ok().content_type("image/jpeg").body(buffer)
    } else {
        HttpResponse::NotFound().json(Value::String("Image not found.".to_string()))
    }
}

// Get file list
#[actix_web::get("/api/list/{subfolder}")]
async fn get_list(
    subfolder: web::Path<String>,
    data: web::Data<Vec<String>>,
) -> impl Responder {
    let subfolder = subfolder.into_inner();
    let data = data[0..data.len() - 2].to_vec();
    let filtered_images: Vec<&String> = if subfolder == "all" {
        data.iter().collect()
    } else {
        data.iter()
            .filter(|&path| path.contains(&subfolder))
            .collect()
    };

    if filtered_images.is_empty() {
        return HttpResponse::NotFound().json(Value::String("No images found.".to_string()));
    }

    let mut file_list = Vec::new();
    for image in filtered_images {
        // Only return filename
        let file_name = Path::new(image)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        file_list.push(file_name);
    }

    HttpResponse::Ok().json(file_list)
}

#[actix_web::post("/api/images/{subfolder}")]
async fn upload_image(
    mut payload: Multipart,
    subfolder: web::Path<String>,
    data: web::Data<Vec<String>>,
    req: HttpRequest,
) -> Result<HttpResponse, Error> {
    let token = data[data.len() - 1].clone();
    // Record the ip address of the visitor
    let ip_str = if let Some(cf_ip) = req.headers().get("CF-Connecting-IP") {
        cf_ip.to_str().unwrap_or("").to_string() // Convert to String
    } else if let Some(peer_addr) = req.peer_addr() {
        peer_addr.ip().to_string()
    } else {
        "".to_string() // Could not get the ip address
    };
    let country = if let Some(cf_country) = req.headers().get("CF-IPCountry") {
        cf_country.to_str().unwrap_or("Unknown country").to_string()
    } else {
        "Unknown country".to_string()
    };
    // Check authentication, should be Bearer <token>
    let auth_header = req.headers().get("Authorization");
    if auth_header
        != Some(
            &actix_web::http::header::HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        )
    {
        println!(
            "Unauthorized access from IP: {}, Country: {}",
            ip_str, country
        );
        return Err(actix_web::error::ErrorUnauthorized("Unauthorized."));
    }
    // Get the folder path from the data
    let image_folder = data[data.len() - 2].clone();
    while let Ok(Some(mut field)) = payload.try_next().await {
        let content_disposition = field.content_disposition();
        let filename = match content_disposition.get_filename() {
            Some(name) => name,
            None => return Err(actix_web::error::ErrorBadRequest("No filename found.")),
        };
        // Get the subfolder from the path as a string
        let subfolder = subfolder.clone();
        let folder_path = format!("{}/{}", image_folder, subfolder);

        fs::create_dir_all(&folder_path)?;

        let mut hasher = Md5::new();
        hasher.update(filename.as_bytes());
        let hash_result = hasher.finalize();
        let new_filename = format!("{:x}.webp", hash_result);
        let new_filepath = format!("{}/{}", folder_path, new_filename);

        let mut buffer = Vec::new();
        // Read the data from the field
        while let Some(chunk) = field.next().await {
            let data = chunk?;
            buffer.extend_from_slice(&data);
        }

        // Load the image from the buffer
        let img = ImageReader::new(std::io::Cursor::new(buffer))
            .with_guessed_format()
            .expect("Failed to guess image format")
            .decode()
            .expect("Failed to decode image");

        // Save the image to the file
        match img.save_with_format(new_filepath.clone(), ImageFormat::WebP) {
            Ok(_) => {
                println!("Image uploaded from  saved to {}", new_filepath);
                match create_thumbnail(Path::new(&new_filepath), 200, 200, &image_folder) {
                    Ok(_) => {
                        println!("Created thumbnail for {new_filepath}");
                    }
                    Err(e) => {
                        eprintln!("Failed to create thumbnail: {e}");
                        return Err(actix_web::error::ErrorInternalServerError(
                            "Image uploaded successfully, but failed to create thumbnail.",
                        ));
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to save image: {}", e);
                return Err(actix_web::error::ErrorInternalServerError(
                    "Failed to save image.",
                ));
            }
        }
    }
    Ok(HttpResponse::Ok().json("Images uploaded successfully."))
}

#[actix_web::get("/api/images/{subfolder}")]
async fn list_images(
    subfolder: web::Path<String>,
    data: web::Data<Vec<String>>,
    req: HttpRequest,
) -> impl Responder {
    let data = data[0..data.len() - 2].to_vec();
    // Get the visitor's ip address and print to log
    let ip_str = if let Some(cf_ip) = req.headers().get("CF-Connecting-IP") {
        cf_ip.to_str().unwrap_or("").to_string() // Convert to String
    } else if let Some(peer_addr) = req.peer_addr() {
        peer_addr.ip().to_string()
    } else {
        "".to_string() // Could not get the ip address
    };

    // Get the visitor's country and print to log
    let country = if let Some(cf_country) = req.headers().get("CF-IPCountry") {
        cf_country.to_str().unwrap_or("Unknown country").to_string()
    } else {
        "Unknown country".to_string()
    };

    println!(
        "Visitor IP: {}, Country: {}, Subfolder: {}",
        ip_str, country, subfolder
    );

    let subfolder = subfolder.into_inner();
    let filtered_images: Vec<&String> = if subfolder == "all" {
        data.iter().collect()
    } else {
        data.iter()
            .filter(|&path| path.contains(&subfolder))
            .collect()
    };

    if filtered_images.is_empty() {
        return HttpResponse::NotFound().json(Value::String("No images found.".to_string()));
    }

    let mut rng = rand::thread_rng();
    let random_index = rng.gen_range(0..filtered_images.len());
    let random_image = filtered_images.get(random_index).unwrap();

    let mut file = File::open(random_image).unwrap();
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).unwrap();

    HttpResponse::Ok().content_type("image/jpeg").body(buffer)
}

#[actix_web::main] // <- Start actix-web
async fn main() -> std::io::Result<()> {
    let config = read_config("config.json");

    // Print the config
    println!("Config: {:?}", config);

    // Validate the image folder
    match validate_folder(&config.image_folder) {
        Ok(_) => println!("Image folder validated."),
        Err(e) => {
            eprintln!("Failed to validate image folder: {}", e);
            // ask the user if they want to create the folder, wait for 3 seconds, default to no
            let mut input = String::new();
            println!("Do you want to create the folder? (y/n)");
            std::io::stdin().read_line(&mut input).unwrap();
            if input.trim() == "y" {
                match create_folder_structure(&config.image_folder) {
                    Ok(_) => println!("Folder created."),
                    Err(e) => {
                        eprintln!("Failed to create folder: {}", e);
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            "Failed to create image folder.",
                        ));
                    }
                }
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Image folder invalid.",
                ));
            }
        }
    }

    // Convert the images to webp format
    match convert_images_to_webp(&config.image_folder) {
        Ok(count) => println!("{} images converted to webp.", count),
        Err(e) => eprintln!("Failed to convert images: {}", e),
    }

    // Create thumbnails
    match create_thumbnails(&config.image_folder, 200, 200, &config.image_folder) {
        Ok(count) => println!("{} thumbnails created.", count),
        Err(e) => eprintln!("Failed to create thumbnails: {}", e),
    }

    let images = index_images(&config.image_folder);

    // Print the number of images indexed
    println!("Indexed {} images.", images.len());

    let image_folder = config.image_folder.clone();
    let mut data_vec = images.clone();
    let token = BASE64_STANDARD.encode(config.pwd.as_bytes());
    data_vec.push(image_folder);
    data_vec.push(token);

    // Attempt to bind the server to the provided address
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(data_vec.clone()))
            .route("/", web::get().to(|| async { "Hello, world!" }))
            .service(list_images)
            .service(upload_image)
            .service(get_thumbnail)
            .service(get_list)
            .service(get_image)
    })
    .bind(format!("{}:{}", config.host, config.port));

    // Check if the server was successfully bound
    match server {
        Ok(server) => {
            println!("Server running at http://{}:{}", config.host, config.port); // Print a success message
            server.run().await // Start the server
        }
        Err(e) => {
            println!("Failed to bind server: {}", e); // Print an error message
            std::process::exit(1); // Exit the program
        }
    }
}
