use actix_web::{web, App, HttpServer};
use base64::prelude::*;

mod handler;
mod utils;

use handler::*;
use utils::*;


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
    println!("{} images indexed. PC: {}, MP: {}", images[0].len() + images[1].len(), images[0].len(), images[1].len());

    let image_folder = config.image_folder.clone();
    let token = BASE64_STANDARD.encode(config.pwd.as_bytes());
    let config_vec = vec![image_folder, token];

    let mut data_vec = images.clone();
    data_vec.push(config_vec);
    
    

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
