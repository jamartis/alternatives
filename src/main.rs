#![allow(unused)]
//use std::fsrust enum set value based on variant;
use std::env;
//use std::collections::HashMap;
//use std::os::unix::fs;
use std::cmp::Ordering;
use std::path::Path;
use std::path::PathBuf;
use std::io::Write;
use std::fs::File;
use std::fs::read_to_string;
use std::fs;
//these two are used to read the alternatives DB, in the future it would be better to write a custom parser, so only the std is used
use serde_json;
use serde::{Deserialize, Serialize};

enum Message {
    Debug {message: String},    
    Info {message: String},    
    Warning {message: String},    
    Error {message: String},    
}

#[derive(PartialEq,PartialOrd,Debug, Clone)]
enum Verbosity {
    Error,
    Warning,
    Info,
    Debug,
}

#[derive(PartialEq,Debug)]
enum Errors {
    Unknown,
    Unimplemented,
    EOK,
    MissingArguments,
    WrongInstallArguments,
    WrongRemoveArguments,
    WrongExportArguments,
    WrongImportArguments,
    UnknownArgument,
    UnknownCommand,
    UnknownInstallOptionalArgument,
    MissingFamilyParameter,
    MissingFollowerParameter,
    MissingInitScriptParameter,
    DBPermissions,
    DBFileNotFound,
    AlternativeAlreadyExists,
}

#[derive(PartialEq,Debug)]
enum DbType {
    Dropin,
    Cli,
}

#[derive(PartialEq,Debug,Serialize,Deserialize,Clone)]
struct Alternative {
    /*
     * The alternative struct contains the "header-like" attributes.
     * The actual file/link/etc names are stored in the Records structs.
     *
     * */
    name: String,
    identifier: String, //For compatibility reasons, the path of the "leader" is used by default for the cli operations
    priority: i32,
    records: Vec<Records>, //theese can be alterantives, initscripts etc
    //These are for internal use only and should not appear in the resulting db files
    db_file: Option<PathBuf>,  

}

#[derive(PartialEq,Debug,Serialize,Deserialize,Clone)]
enum Records {
    File {
        link: String,
        name: String,   
        path: String,
    },
    Initscript,
}
impl Records {
    fn to_json (&self) -> String {
        let mut rv = "".to_string();
        match &self {
            Records::File {link,name,path} => {
                rv.push_str(&format!("      {{\n"));
                rv.push_str(&format!("        \"File\": {{\n"));
                rv.push_str(&format!("          \"link\": \"{}\",\n", link)); 
                rv.push_str(&format!("          \"name\": \"{}\",\n", name));
                rv.push_str(&format!("          \"path\": \"{}\"\n", path));
                rv.push_str(&format!("        }}\n"));
                rv.push_str(&format!("      }}\n"));
            },
            _ => {},
        }
        return rv;
    }
}

impl PartialOrd for Alternative {
    // TODO - this might need some more refinement, e.g. should alternatives with diffent names be comparable?
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if (self.priority < other.priority) {
            return Some(Ordering::Less)
        } else if (self.priority > other.priority) {
            return Some(Ordering::Greater)
        }
        
        // The priorities are equal, identifier should now decide
        // Identifier should be unique among the alternatives with the same name,
        // the equal branch should thus only be reached when comapring alternatives with different names
        if (self.identifier < other.identifier) {
            return Some(Ordering::Less)
        } else if (self.identifier > other.identifier) {
            return Some(Ordering::Greater)
        }

        return None;
        
    }
}

impl Alternative {
    fn new (name: String, identifier: String, prio: i32, db_file: Option<PathBuf>) -> Self {
        Self {
            name: name,
            identifier: identifier,
            priority: prio,
            records: Vec::new(),
            db_file: db_file,
        }
    }

    fn to_json (&self) -> String {
        let mut rv = "".to_string();
        rv.push_str(&format!("  {{\n"));
        rv.push_str(&format!("    \"name\": \"{}\",\n",self.name)); 
        rv.push_str(&format!("    \"identifier\": \"{}\",\n",self.identifier));
        rv.push_str(&format!("    \"priority\": {},\n", self.priority));
        rv.push_str(&format!("    \"records\": [\n"));

        let mut comma = false;
        for rec in &self.records {
            if comma == true {
                rv.push_str(&format!("        ,\n")); //TODO -> put the comma on the previous line to make the json a bit prettier
            }
            rv.push_str(&rec.to_json());
            comma = true;
        }

        
        rv.push_str(&format!("    ]\n"));
        rv.push_str(&format!("  }}\n"));
        return rv;

    }

    
    fn uninstall (env: &Settings, name: String, identifier: String) -> Result<(),Errors> {
        // We need to:
        // Load the DB
        // Check, whether the removed alternative has the highest prio
        // If it does set the new highest alterntive
        // disable the alternative
        // update the DB;

        //Read the DB
        let db_file_name = env.get_db_file_name(&name);
        print_message(env, Message::Debug{message: format!("Reading db file: {:#?}\n",db_file_name).to_string()});
        let mut cli_alternatives = read_db_file(env, &db_file_name)?;

        // Find the alternative to be removed
        let alt = match Alternative::get_alternative(env,&cli_alternatives, &identifier) {
            None => {return Err(Errors::Unimplemented);}
            Some(x) => {x}
        };
        print_message(env,Message::Debug{message: format!("Found the alternative:\n{:#?}",alt)});

        

        //remove it from the loaded db
        cli_alternatives = cli_alternatives.into_iter().filter(|x| x.identifier != identifier).collect();
        // Find the highest one in the new DB
        let highest_cli = Alternative::highest_prio(env, &cli_alternatives).clone();
        print_message(env,Message::Debug{message: format!("The new highest prio:{:#?}",highest_cli)});

        // Write the new DB
        print_message(env,Message::Debug{message: format!("The new vector of alternatives: {:#?}\n",&cli_alternatives)});
        if highest_cli == None {
            // There are no alternatives left -> just remove the file
            if (alt.db_file == None) {               
            }
            else {
                remove_db_file(env,alt.db_file.clone().unwrap())?
            };
        } else {
            //there're still some alternatives
            write_db_file(env, &cli_alternatives)?;
        }


        match highest_cli {
            None => {
                for record in alt.records {
                    disable_record(env, &record )?; // TODO make sure these are full paths 
                }
            } // No remaining alternatives -- disabel this one
            Some(h) if h > alt => {} // Wasn't the highest -- no updates
            Some(h) => {//Was the highest
                for record in alt.records {
                    disable_record(env, &record )?; // TODO make sure these are full paths 
                }
                for record in h.records {
                    enable_record(env, &record )?; // TODO make sure these are full paths 
                }
            }
        }

        

        return Ok(());
    }

    /*
     * This function is THE function for installation of a new alterantive
     * It is among other responsible for:
     * - checking the DB files for conflicts
     * - updating the DB files
     * - removing the symlink obsolated by the installed alternative
     * - creating the new symlinks
     * */
    fn install (&self,env: &Settings) -> Result<(),Errors> {
        // Open the DB file, the file path name si based on the alternative name
        let db_file_name = env.get_db_file_name(&self.name);

        print_message(env, Message::Debug{message: format!("Reading db file: {:#?}\n",db_file_name).to_string()});
        let mut cli_alternatives = match read_db_file(env, &db_file_name) {
            Ok(alt) => {alt}
            _ => {Vec::new()}
        };

        // Check for possible conflicts
        // Important: the alternative's name and identifier combination must be unique
        // Less important: multiple alterantives (mostly via followers) managing overlapping set of symlinks //TODO NOT IMPLEMNTED
        self.check_conflicts(env, &cli_alternatives)?; //Abort the installation in case of errors

        //Find an existing alternative with the highest priority
        let highest_cli = Alternative::highest_prio(env, &cli_alternatives).clone();
        print_message(env,Message::Debug{message: format!("The highest cli prio: {:#?}\n",highest_cli)});

        //update the DB
        cli_alternatives.push(self.clone());
        print_message(env,Message::Debug{message: format!("The new vector of alternatives: {:#?}\n",&cli_alternatives)});
        write_db_file(env, &cli_alternatives);

        // Update the symlinks        
        match highest_cli {
            // The simplest case -> we only need to add the new alternative to the DB, no links shall be modified
            Some(h) if &h >= self => {
                //Nothing ot do
            }
            // The new alternative has highest priority -> we need to add it to the db + update the links. There are two subcase:
            // There was some alternative before or...
            Some(h) => {
                // Disable the previous alternative
                for record in &h.records {
                    disable_record(env, &record )?; // TODO make sure these are full paths 
                }

                // And enable the new one
                for record in &self.records {
                    enable_record(env, &record)?;
                }
            }
            // ...this is the first alternative of its name
            None => {
                // And enable the new one
                for record in &self.records {
                    enable_record(env, &record)?;
                }
            }
        }
        return Ok(());
    }

    // Input: a json file (using the alternatives structure) containing a vector of alternatives
    // This function the installs all the alternatives from the input file
    // This should proabably be done in two passes
    // - first do a dry run, if it ends up with some fatal errors, jsut abort
    // - second run -> just call the install method
    fn dropin_import (env: &Settings, path: PathBuf) -> Result<(),Errors> {
        // First check for possible conflicts, withouy actually installing anything
        let mut drop_in = read_db_file (env, &path)?;
        for alt in &drop_in {
            // TODO do the checks before actually intalling anything
        }

        // Now just call the install method for each alternative
        for alt in &mut drop_in {
            let db_file_name = env.get_db_file_name(&alt.name);
            alt.db_file = Some(db_file_name);
            alt.install(env);
        }
        
        return Ok(());
    }

    // Reverse function to the dropin_import - uninstall the alternatives specified in the input file (path)
    // TODO add an option for more complex identity checks, right now only name and identifier are used. An option to compare all the fields might be useful
    fn dropin_remove (env: &Settings, path: PathBuf) -> Errors {
        return Errors::Unimplemented;
    }

    // Prints the alternatives specified by the alts argument in the json format
    // The DB files/symlinks remain unchanged
    fn export_alternatives (env: &Settings,alts: Vec<(String/*alternative*/,String/*identifier*/)>) -> Result<(),Errors> {
        let mut exported_alts: Vec<Alternative> = vec![];
        for (alt,id) in alts {
            print_message(env,Message::Debug{message: format!("Processing: {:#?}:{:#?}\n",alt,id)});
            let db_file_name = env.get_db_file_name(&alt);
            let mut cli_alternatives = read_db_file(env, &db_file_name)?; // TODO should a nonexistant alternative be a fatal error?
            match cli_alternatives.into_iter().find(|x| x.identifier == id) {
                None => {
                    print_message(env,Message::Debug{message: format!("Alternative: {:#?}:{:#?} not found\n",alt,id)});
                }
                Some(a) => {
                    print_message(env,Message::Debug{message: format!("Alternative found: {:#?}:{:#?}\n{:#?}",alt,id,a)});
                    exported_alts.push(a.clone());
                }
            } 
            
        }
        print!("{}",alts_to_json(env, &exported_alts));
        return Ok(());
    }
    
    fn get_alternative (env: &Settings,alts: &Vec<Alternative>, identifier: &String) -> Option<Alternative> {
        for alt in alts {
            if alt.identifier == *identifier {
                return Some(alt.clone());
            }
        }     
        return None;
    }

    fn highest_prio (env: &Settings, alts: &Vec<Alternative>) -> Option<Alternative> {
        let mut rv = None;
        for alt in alts {
            match rv {
                None => {rv = Some(alt.clone())}
                Some(h) if *alt > h => {rv = Some(alt.clone())}
                _ => {}
            }
        }
        return rv;
    }
    
    fn check_conflicts (&self, env: &Settings, alts: &Vec<Alternative>) -> Result<(),Errors> {
        // The simplest check -> make sure the name/identifier combination is unique. (Fatal error if not)
        // TODO add argument that would allow overwriting and downgrade this to a warning/info messages
        let dupl_alternatives: Vec<_> = alts.into_iter().filter(|x| x.identifier == self.identifier && x.name == self.name).collect();
        if dupl_alternatives.is_empty() == false {
            for dupl in dupl_alternatives {
                print_message(env,Message::Error{message: format!("The specified alternative already exists! (Use the \"info\" verbosity level for more details)\n")});
                print_message(env,Message::Info{message: format!("The existing alternative:\n{:#?}",dupl)});
            }
            return Err(Errors::AlternativeAlreadyExists);
        }

        //TODO: check for the same symlink being modified by multiple alternatives 

        return Ok(())        
    }
}

#[derive(PartialEq,Debug)]
enum Command {
    Install{alternative: Alternative},
    InstallExport{alternative: Alternative},
    Remove{name: String, path: String},
    Auto{name: String},
    Export{alternatives: Vec<(String,String)>},
    Import{paths: Vec<PathBuf>},
    Help,
    None,
}


fn enable_record (env: &Settings, rec: &Records) -> Result<(),Errors> {
    match rec {
        Records::File{link, name, path} => {
            print_message(env, Message::Info{message: format!("Linking: {:#?} -> {:#?}",link,name).to_string()});
            if env.dry_run == false {
                //TODO -> the actual fs operation
            }

            print_message(env, Message::Info{message: format!("Linking: {:#?} -> {:#?}",name,path).to_string()});
            if env.dry_run == false {
                //TODO -> the actual fs operation
            }
            return Ok(());
        }
        _ => {
            print_message(env, Message::Error{message: format!("This operation is not yet implemented!\n").to_string()});
            return Err(Errors::Unimplemented);
            
        }
    }    
}

fn disable_record (env: &Settings, rec: &Records) -> Result<(),Errors> {
    match rec {
        Records::File{link, name, path} => {
            print_message(env, Message::Info{message: format!("Unlinking: {:#?}",link).to_string()});
            if env.dry_run == false {
                //TODO -> the actual fs operation
            }

            print_message(env, Message::Info{message: format!("Unlinking: {:#?}",name).to_string()});
            if env.dry_run == false {
                //TODO -> the actual fs operation
            }
            return Ok(());
        }
        _ => {
            print_message(env, Message::Error{message: format!("This operation is not yet implemented!\n").to_string()});
            return Err(Errors::Unimplemented);
            
        }
    }    
}


fn print_help() {
    println!("Help Message:") ;

}


fn print_message (env: &Settings, mes: Message) {
    match mes {
           Message::Error{message} => {
                if env.verbosity >= Verbosity::Error {
                    let mes: String = message.lines().map(|x| format!("Error: {}\n",x)).collect();
                    println!("{}",mes);
                }
            }
            Message::Warning{message} => {
                if env.verbosity >= Verbosity::Warning {
                    let mes: String = message.lines().map(|x| format!("Warning: {}\n",x)).collect();
                    println!("{}",mes);
                }
            }
            Message::Info{message} => {
                if env.verbosity >= Verbosity::Info {
                    let mes: String = message.lines().map(|x| format!("Info: {}\n",x)).collect();
                    println!("{}",mes);
                }
            }
            Message::Debug{message} => {
                if env.verbosity >= Verbosity::Debug {
                    let mes: String = message.lines().map(|x| format!("Debug: {}\n",x)).collect();
                    println!("{}",mes);
                }
            }
        }
}

fn alts_to_json (env: &Settings, alts: &Vec<Alternative>) -> String {
    
    let mut content = "".to_string();
    content.push_str(&format!("[\n"));
    let mut comma = false;
    for alt in alts {
        if comma == true {
            content.push_str(&format!("        ,\n")); //TODO -> put the comma on the previous line to make the json a bit prettier
        }
        content.push_str(alt.to_json().as_str());
        comma = true;
    }
    content.push_str(&format!("]\n"));
    return content;
}

// Check the db directory and get a list of all .json files there
fn list_db_files (env: &Settings, path_name: PathBuf) -> Result<Vec<PathBuf>,Errors> {

    print_message(env,Message::Debug{message: format!("Setting path to: {:#?}",path_name)});

    let dir_path = Path::new(&path_name);
    let entries = match fs::read_dir(dir_path) {
        Ok(entries)  => {entries}
        Err(why) => {return Err(Errors::Unimplemented);}
    };

    // Filter out all entries with the wrong extension
    // TODO Depending on how we want to recover from possible errors, this might be simplified by using collect()
    // For now we try to handle each error individualy 
    let mut rv = Vec::new();
    for entry in entries {
       print_message(env,Message::Debug{message: format!("Checking entry: {:#?}",entry)});
       match entry {
           Ok(ref e) => {
               match (e.path().extension()) {
                   None => { //skip this entry
                           print_message(env,Message::Debug{message: format!("Skipping entry: {:#?}",entry)});
                   }
                   Some(ext) => {
                       print_message(env,Message::Debug{message: format!("Extension: {:#?}",ext)});
                       if (ext =="json") {
                           print_message(env,Message::Debug{message: format!("Adding path: {:#?}",e.path())});
                           rv.push(e.path());
                       }
                   }
               }
           }
           Err(why) => {
               print_message(env,Message::Error{message: format!("Error: {:#?}",why)});//TODO
               return Err(Errors::Unimplemented);
           }
       } 
    }
    return Ok(rv);
}

fn read_db_file (env: &Settings, file_name: &PathBuf) -> Result<Vec<Alternative>,Errors> {
    let mut rv: Vec<Alternative> = Vec::new();
    //TODO consider the buffered approach https://doc.rust-lang.org/rust-by-example/std_misc/file/read_lines.html 
    let f = match read_to_string(file_name) {
        
        Ok(file) => {file}
        #[allow(non_snake_case)] //TODO check this
        NotFound => {
            print_message(env,Message::Info{message: format!("DB file not found:{:#?}",file_name)});
            return Err(Errors::DBFileNotFound);
        }
        #[allow(non_snake_case)] //TODO check this
        PermissionDenied => {
            print_message(env,Message::Error{message: format!("File permissions error:{:#?}",file_name)});
            return Err(Errors::DBPermissions);
        }
        _ =>  {return Err(Errors::Unknown);}

    };
    
    match serde_json::from_str(&f) {
        Err(why) => {dbg!(why); return Err(Errors::Unknown);}
        Ok(alts) => {rv = alts;}
    };
    for alt in rv.iter_mut() {
        alt.db_file = Some(file_name.clone());
    }

    print_message(env,Message::Debug{message: format!("Read DB file:\n{:#?}",rv)});
    return Ok(rv);
}

fn remove_db_file(env: &Settings, path: PathBuf) -> Result<(),Errors> {

    print_message(env,Message::Debug{message: format!("Removing File:{:#?}",path)});
    match fs::remove_file(path) {
            Ok(_) => {return Ok(())}
            _ => {return Err(Errors::Unimplemented)}
    }
}

fn write_db_file(env: &Settings, alts: &Vec<Alternative>) -> Result<(),Errors> {
    /*
     * The function expects that all the alternatives are to be written to the same file (determined by the first aternative in the list)
     */
    if alts.is_empty() {
        return Ok(());
    }

    let file_name;
    if let Some(f) = &alts.first().unwrap().db_file {
        file_name = f.clone();
    }
    else {
        return Err(Errors::Unknown);
    };

    /*
     * TODO: backup the original file
     */

    let path = file_name.as_path();

    
    print_message(env,Message::Info{message: format!("Updating file {:#?}",file_name)});

    let content = alts_to_json(&env, alts);
    if env.dry_run == false {
        let mut file = match File::create(&path) {
            Err(why) => return Err(Errors::Unknown),
            Ok(file) => file,
        };
        print_message(env,Message::Debug{message: format!("Writing:\n{}\n",content)});
        if env.dry_run == false {
            match file.write_all(content.as_bytes()) {
                Err(why) => return Err(Errors::Unknown),
                Ok(_) => {},
            }
        }
    } else {
        print_message(env,Message::Debug{message: format!("Writing:\n{}\n",content)});
    }

    return Ok(());
}

#[derive(Clone,Debug)]
pub struct Settings {
    verbosity: Verbosity,
    dry_run: bool,
    path_prefix: String,
    admin_dir: String,
    alternatives_dir: String,
}

impl Default for Settings {
    fn default () -> Self {
        Settings {
            verbosity: Verbosity::Warning,
            dry_run: true,
            path_prefix: "".to_string(),
            admin_dir: "/tmp".to_string(),
            alternatives_dir: "/tmp".to_string(),
        }    
    }
}

impl Settings {
    fn get_db_cli_dir (&self) -> PathBuf {
       return PathBuf::from(format!("{}/cli/",self.admin_dir)); 
    }
    
    fn get_db_file_name (&self,alt: &String) -> PathBuf {
       return PathBuf::from(format!("{}/cli/{}.json",self.admin_dir,alt)); 
       //return format!("{}/cli/{}.json",self.admin_dir,alt); 
    }

    fn parse_args (args: Vec<String>) -> Result<(Command, Settings),Errors> {

        let mut rv_command = Command::None;
        let mut rv_settings: Settings = Default::default();
        if args.len() == 1 {
            return Err(Errors::MissingArguments);
        }
        /*
         * Lets deal with the common options first, when done, i should be the index of the command
         */
        let mut i = 1;
        loop {
            if i == args.len() {
                    return Err(Errors::UnknownArgument);
            }
            match args[i].as_str() {
                //TODO: use the --verbosit=foo syntax instead
                "--debug"  => {rv_settings.verbosity = Verbosity::Debug}
                //TODO: use the --verbosit=foo syntax instead
                "--verbose"  => {rv_settings.verbosity = Verbosity::Info}
                "--dry-run"  => {rv_settings.dry_run = true}
                "--no-dry-run"  => {rv_settings.dry_run = false}
                /*
                 * The commnands start here, we've already read the common options, so the command specific parsing should be doen next -> breaking the initial loop
                 */
                "--install"  => {break;}
                "--install-batch"  => {break;} // installs from the given json file
                "--import" => {break;} // alias of the previous
                "--uninstall-batch"  => {break;} // uninstalls based on the given json file
                "--remove-batch"  => {break;} //alias of the previous
                "--uninstall"  => {break;}
                "--remove"  => {break;} // alias of the previous
                "--export"  => {break;} 
                "--install-export"  => {break;} // same syntax as install, but instead of installing converts the argument to json and prints it
                "--convert"  => {break;} // Converts the file from the original alternatives format to json and prints it (experimental feature, the resulting output should be reviewed and direct installation is thus not supported)

                    _ => {return Err(Errors::UnknownArgument);}
            }
            i += 1;
            
        }
        //the previous loop ended at one of the commands:
        match args[i].as_str() {
            "--import" | "--install-batch" => {
                let mut paths: Vec<PathBuf> = vec![];
                loop { //there should now be a list of files to import
                    if let Some(_) = args.get(i+1) {
                        let path = match PathBuf::from(args[i+1].clone()).canonicalize() {
                            Ok(p) => {p}
                            Err(why) => {
                                // TODO error message
                                return Err(Errors::WrongImportArguments)
                            }
                        };
                        paths.push(path); // TODO check the PathBuf::new return value
                    }
                    else {
                        break; //no more arguments to parse
                    }
                    i+=1;
                }
                if paths.is_empty() == false {
                    let rv_command = Command::Import{paths: paths};
                    return Ok((rv_command,rv_settings.clone()));
                } else {
                   return Err(Errors::WrongImportArguments);
                }
            }
            "--export" => {
                let mut alts: Vec<(String,String)> = vec![];
                loop { //there should now be a list of name identifier pairs (at least one pair)
                    if let Some(_) = args.get(i+2) {
                        alts.push((args[i+1].clone(),args[i+2].clone()));
                    }
                    else {
                        break;
                    }
                    i+=2;
                }
                if alts.is_empty() == false {
                    let rv_command = Command::Export{alternatives: alts};
                    return Ok((rv_command,rv_settings.clone()));
                } else {
                   return Err(Errors::WrongExportArguments);
                }
            }
            "--remove" | "--uninstall" => {
                if let Some(_) = args.get(i+2) { // check there are at least 2 more arguments
                    let rv_command = Command::Remove{
                        name: args[i+1].clone(),
                        path: args[i+2].clone(),
                    };
                    return Ok((rv_command,rv_settings.clone()));
                }
                else {
                   return Err(Errors::WrongRemoveArguments);
                }
            }
            "--install" | "--install-export" => {
                let mut alternative;// = Alternative::new();
                // Check which variant are we actually using, before the 'i' gets modified
                let install_export = args[i] == "--install-export";
                //Check whether there are at least 4 more arguments (the 4th one is the priority)
                if let Some(_prio) = args.get(i+4) {
                    
                    alternative = Alternative::new(
                        args[i+2].clone(),
                        args[i+3].clone(),
                        _prio.parse().unwrap(),
                        Some(PathBuf::from(format!("{}/cli/{}.json",rv_settings.admin_dir,args[i+2]))), //TODO use a function instead
                    );

                    alternative.records.push(Records::File {                    
                        link: args[i+1].clone(),
                        name: args[i+2].clone(),
                        path: args[i+3].clone(),
                    });

                    i += 5;
                } else {
                    return Err(Errors::WrongInstallArguments);
                }
                //check for the optional arguments (followers etc.)
                loop {
                    if let Some(_optional) = args.get(i) {
                        match _optional.as_str() {
                            "--follower" | "--slave" => {
                                if let Some(_path) = args.get(i+3){

                                    alternative.records.push(Records::File {                    
                                        link: args[i+1].clone(),
                                        name: args[i+2].clone(),
                                        path: args[i+3].clone(),
                                    });
                                    i += 4;
                                }
                                else {
                                    {return Err(Errors::MissingFollowerParameter);}
                                }
                            }
                            "--initscript" => {
                                if let Some(_script) = args.get(i+1){
                                    alternative.records.push(Records::Initscript); //TODO
                                    i += 2;
                                }
                                else {
                                    {return Err(Errors::MissingInitScriptParameter);}
                                }
                            }
                            "--family" => {
                                if let Some(_fam) = args.get(i+1){
                                    //TODO
                                    i += 2;
                                }
                                else {
                                    {return Err(Errors::MissingFamilyParameter);}
                                }
                            }
                                _ => {return Err(Errors::UnknownInstallOptionalArgument);}
                        }
                    
                    }
                    //no more arguments
                    else {
                        break;
                    }
                }
                rv_command = if install_export == true
                {
                    Command::InstallExport{alternative: alternative}
                }
                else {
                    Command::Install{alternative: alternative}
                };
                return Ok((rv_command,rv_settings.clone()));

            }
            _ => {return Err(Errors::UnknownCommand);}
        }
    }
}


fn main() {
    let args: Vec<String> = env::args().collect();
    let mut env: Settings = Settings::default();
    
    let (command, env) = match Settings::parse_args(args) {
        Ok ((c,e)) => {(c,e)}
        Err (why) => {
            // TODO pretify this
            dbg!(why);
            panic!();
        }
    };
    let rv = match command {
        Command::Install{alternative} => {
            let ops = alternative.install(&env);
            Errors::EOK
        }
        Command::Remove{name, path} => {
            let ops = Alternative::uninstall(&env,name,path);
            Errors::EOK
        }
        Command::Export{alternatives} => {
            let ops = Alternative::export_alternatives(&env,alternatives);
            Errors::EOK
        }
        Command::Import{paths} => {
            for path in paths {
               Alternative::dropin_import(&env, path); 
            }
            Errors::EOK
        }
        Command::InstallExport{alternative} => {
            let mut aux = vec![];
            aux.push(alternative);
            print!("{}",alts_to_json(&env, &aux));
            Errors::EOK
        }


        _ => {
            
            Errors::Unimplemented
        }
    };

}
