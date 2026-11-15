use datamodels::packets::get_log_metadata;
use datamodels::packets::LogMetadata;

pub fn print_splash_screen() {
    let application_version = option_env!("CARGO_PKG_VERSION").map(|s| s.to_string());
    let log_metadata: LogMetadata = get_log_metadata(application_version);
    println!("{}", log_metadata);

    let ascii_art = r"
                /######\
               /##########\
              /   \###/    \
             /     \#/      \
          /\|               |/\
          | | \ ==\    /== / | |
           \|  \<|>\  /<|>/  |/     /|
    \__     |    -   \  -    |     /#|
     \#\     |        |      |   /###|
      \##\   |       \|     |  /#####|
       \###\  |   _______  | /######|
        \####\ | / \/ \/ \|/#######|
        |######\|        |#########|
        |########\______/##########|
        |#########\    /##########/
        |##########\  |#########/\
        /###########\/########/###\
    /################\######/########\
   /##################\###/###########\
  /###################\#/##############\
 /####################/#################\
/###################/####################\

  ########## -- FIRST BITE -- ##########
    ";

    println!("{}", ascii_art);
    println!("First-Bite captures traffic on the wire and \nconverts it into easy-to-use JSON.\n");
}
