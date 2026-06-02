//#![allow(unused)]

use std::os::unix::fs::symlink; //Create symlink (unlinking is part of the std::fs)
use std::process::ExitCode;
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


/*
 * TODO: The parser for the old format
 * TODO: manual mode
 * TODO: priority manipulation options (at least --top-priority, --least-priority, nice to have: --reset-priorities (keeps the relative ordering, but resets the actual priorities to "sane values"))
 * TODO: initscripts
 * TODO: error recovery options: e.g. when the leader record cannot be created, should the followers be created?
 * TODO: argument parser: read all the remaining arguments even when there's match
 */

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

#[derive(PartialEq,PartialOrd,Debug, Clone)]
enum RunMode {
    Preview,
    DbOnly,
    Full,
    //Force, //Currently not used but might be implemented in the future, that's why the >= Full comparisons are being used instead of plain ==
}

#[derive(PartialEq,Debug)]
enum Errors {
    Unknown = 1, //1
    Unimplemented,
    MissingArguments,
    WrongArguments,
    DBPermissions, //5
    DBFileNotFound,
    DBFileError,
    AdminDirPermissions,
    AltDirPermissions,
    AlternativeAlreadyExists, //10
    AlternativeNotFound,
    Symlink,
    Json,
    InternalError,
    
}

impl Errors {
    fn error_messages (&self) -> &str {
        match &self {
            Errors::Symlink => {"Error during the symlink manipulation."}
            Errors::AdminDirPermissions => {"Could not access the admin directory."}
            Errors::AltDirPermissions => {"Could not access the alternatives directory."}
            Errors::DBPermissions => {"Permissions error encountered while reading the DB file."}
            Errors::DBFileNotFound => {"Db file not found."}
            Errors::DBFileError => {"DB file error."}
            Errors::WrongArguments => {"Wrong arguments have been provided."}
            Errors::MissingArguments => {"Expected arguments have not been provided."}
            Errors::AlternativeAlreadyExists => {"The specified alternative already exists."}
            Errors::AlternativeNotFound => {"The specified alternative has not been found."}
            Errors::Unimplemented => {"This functionality has not been implemented yet."}
            Errors::InternalError => {"Unknown internal error."}
            Errors::Json => {"Error occured while parsing the json file."}
            _ => {"Unknown Error encountered."}
        }     
    }
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
    records: Vec<Records>, //these can be alternatives, initscripts etc
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
    Initscript {
        script: String,
    },
}

impl PartialOrd for Alternative {
    // TODO - this might need some more refinement, e.g. should alternatives with different names be comparable?
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.priority < other.priority {
            return Some(Ordering::Less)
        } else if self.priority > other.priority {
            return Some(Ordering::Greater)
        }
        
        // The priorities are equal, identifier should now decide
        // Identifier should be unique among the alternatives with the same name,
        // the equal branch should thus only be reached when comparing alternatives with different names
        if self.identifier < other.identifier {
            return Some(Ordering::Less)
        } else if self.identifier > other.identifier {
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


    
    fn uninstall (_env: &Settings, name: &str, identifier: &str) -> Result<(),Errors> {
        // We need to:
        // Load the DB
        // Check, whether the removed alternative has the highest prio
        // If it does set the new highest alternative
        // disable the alternative
        // update the DB;

        //Read the DB
        let db_file_name = _env.get_db_file_name(&name);
        print_debug(_env, format!("Reading db file: {:#?}",db_file_name));
        let alternatives = read_db_file(_env, &db_file_name)?;

        // Find the alternative to be removed
        let alt = match Alternative::get_alternative(_env,&alternatives, &identifier) {
            None => {
                return Err(Errors::AlternativeNotFound);
            }
            Some(x) => {x.clone()}
        };
        print_debug(_env, format!("Found the alternative:\n{:#?}",alt));

        

        //remove it from the loaded db
        let new_alternatives: Vec<_> = alternatives.into_iter().filter(|x| x.identifier != identifier).collect();
        // Find the highest one in the new DB
        let highest_prio = Alternative::highest_prio(_env, &new_alternatives);
        print_debug(_env, format!("The new highest prio:{:#?}",highest_prio));

        // Write the new DB
        print_debug(_env, format!("The new vector of alternatives: {:#?}",&new_alternatives));
        if highest_prio == None {
            // There are no alternatives left -> just remove the file
            if alt.db_file == None {               
            }
            else {
                remove_db_file(_env,alt.db_file.clone().unwrap())?
            };
        } else {
            //there're still some alternatives
            write_db_file(_env, &new_alternatives)?;
        }


        match highest_prio {
            None => {
                for record in alt.records {
                    disable_record(_env, &record )?; 
                }
            } // No remaining alternatives -- disable this one
            Some(h) if *h > alt => {} // Wasn't the highest -- no updates
            Some(h) => {//Was the highest
                for record in alt.records {
                    disable_record(_env, &record )?; 
                }
                for record in &h.records {
                    enable_record(_env, &record )?; 
                }
            }
        }

        

        Ok(())
    }

    /*
     * This function is THE function for installation of a new alternative
     * It is among other things responsible for:
     * - checking the DB files for conflicts
     * - updating the DB files
     * - removing the symlink obsoleted by the installed alternative
     * - creating the new symlinks
     * */
    fn install (&self,_env: &Settings) -> Result<(),Errors> {
        // Open the DB file, the file path name is based on the alternative name
        let db_file_name = _env.get_db_file_name(&self.name);

        print_debug(_env, format!("Reading db file: {:#?}",db_file_name));
        let mut cli_alternatives = match read_db_file(_env, &db_file_name) {
            Ok(alt) => {alt}
            Err(Errors::Json) => {return Err(Errors::Json)}
            _ => {Vec::new()}
        };

        // Check for possible conflicts
        // Important: the alternative's name and identifier combination must be unique
        // Less important: multiple alternatives (mostly via followers) managing overlapping set of symlinks //TODO NOT IMPLEMENTED
        self.check_conflicts(_env, &cli_alternatives)?; //Abort the installation in case of errors

        //Find an existing alternative with the highest priority

        //update the DB
        cli_alternatives.push(self.clone());
        print_debug(_env, format!("The new vector of alternatives: {:#?}",&cli_alternatives));
        let highest_cli = Alternative::highest_prio(_env, cli_alternatives.split_last().unwrap().1); //since we just pushed into the vector, it should not be empty and thus the unwrap should be safe
        print_debug(_env, format!("The highest prio: {:#?}",highest_cli));
        write_db_file(_env, &cli_alternatives)?;

        // Update the symlinks        
        match highest_cli {
            // The simplest case -> we only need to add the new alternative to the DB, no links shall be modified
            Some(h) if h >= self => {
                //Nothing to do
            }
            // The new alternative has highest priority -> we need to add it to the db + update the links. There are two subcases:
            // There was some alternative before or...
            Some(h) => {
                // Disable the previous alternative
                for record in &h.records {
                    disable_record(_env, &record )?; 
                }

                // And enable the new one
                for record in &self.records {
                    enable_record(_env, &record)?;
                }
            }
            // ...this is the first alternative of its name
            None => {
                // And enable the new one
                for record in &self.records {
                    enable_record(_env, &record)?;
                }
            }
        }
        Ok(())
    }

    // Input: a json file (using the alternatives structure) containing a vector of alternatives
    // This function then installs all the alternatives from the input file
    // This should probably be done in two passes
    // - first do a dry run, if it ends up with some fatal errors, just abort
    // - second run -> just call the install method
    fn dropin_import (_env: &Settings, path: PathBuf) -> Result<(),Errors> {
        // First check for possible conflicts, without actually installing anything
        print_debug(_env, format!("Reading the dropin file\n{:#?}", &path));
        let mut drop_in = read_db_file (_env, &path)?;
        /*for alt in &drop_in {
            // TODO do the checks before actually installing anything
        }*/

        // Now just call the install method for each alternative
        for alt in &mut drop_in {
            let db_file_name = _env.get_db_file_name(&alt.name);
            alt.db_file = Some(db_file_name);
            alt.install(_env)?;
        }
        
        Ok(())
    }

    // Reverse function to the dropin_import - uninstall the alternatives specified in the input file (path)
    // TODO add an option for more complex identity checks, right now only name and identifier are used. An option to compare all the fields might be useful
    fn dropin_remove (_env: &Settings, path: PathBuf) -> Result<(),Errors> {
        print_debug(_env, format!("Reading the dropin file\n{:#?}", &path));
        let drop_in = read_db_file (_env, &path)?;
        for alt in drop_in {
            Alternative::uninstall(_env, &alt.name, &alt.identifier)?;
        }
        Ok(())
    }

    // Prints the alternatives specified by the alts argument in the json format
    // The DB files/symlinks remain unchanged
    fn export_alternatives (_env: &Settings,alts: Vec<(String/*alternative*/,String/*identifier*/)>) -> Result<(),Errors> {
        let mut exported_alts: Vec<Alternative> = vec![];
        for (alt,id) in alts {
            print_debug(_env, format!("Processing: {:#?}:{:#?}",alt,id));
            let db_file_name = _env.get_db_file_name(&alt);
            let cli_alternatives = read_db_file(_env, &db_file_name)?; // TODO should a nonexistent alternative be a fatal error?
            match cli_alternatives.into_iter().find(|x| x.identifier == id) {
                None => {
                    print_debug(_env, format!("Alternative: {:#?}:{:#?} not found",alt,id));
                }
                Some(a) => {
                    print_debug(_env, format!("Alternative found: {:#?}:{:#?}\n{:#?}",alt,id,a));
                    exported_alts.push(a.clone());
                }
            } 
            
        }
        print!("{}",&exported_alts.to_json(_env));
        Ok(())
    }
    
    fn get_alternative<'a> (_env: &Settings,alts: &'a Vec<Alternative>, identifier: &str) -> Option<&'a Alternative> {
        for alt in alts {
            if alt.identifier == *identifier {
                return Some(alt);
            }
        }     
        None
    }

    fn highest_prio<'a> (_env: &Settings, alts: &'a[Alternative]) -> Option<&'a Alternative> {
        let mut rv = None;
        for alt in alts {
            match rv {
                None => {rv = Some(alt)}
                Some(h) if alt > h => {rv = Some(alt)}
                _ => {}
            }
        }
        rv
    }
    
    fn check_conflicts (&self, _env: &Settings, alts: &Vec<Alternative>) -> Result<(),Errors> {
        // The simplest check -> make sure the name/identifier combination is unique. (Fatal error if not)
        // TODO add argument that would allow overwriting and downgrade this to a warning/info messages
        let dupl_alternatives: Vec<_> = alts.into_iter().filter(|x| x.identifier == self.identifier && x.name == self.name).collect();
        if !dupl_alternatives.is_empty() {
            for dupl in dupl_alternatives {
                print_error(_env, format!("The specified alternative already exists! (Use the \"info\" verbosity level for more details)"));
                print_info(_env, format!("The existing alternative:\n{:#?}",dupl));
            }
            return Err(Errors::AlternativeAlreadyExists);
        }

        //TODO: check for the same symlink being modified by multiple alternatives 

        Ok(())        
    }
}

#[derive(PartialEq,Debug)]
enum Command {
    Install{alternative: Alternative},
    InstallExport{alternative: Alternative},
    Remove{name: String, path: String},
    //Auto{name: String},
    Export{alternatives: Vec<(String,String)>},
    Import{paths: Vec<PathBuf>},
    UninstallBatch{paths: Vec<PathBuf>},
    Help,
}

fn enable_record (_env: &Settings, rec: &Records) -> Result<(),Errors> {
    match rec {
        Records::File{link, name, path} => {
            let full_name = _env.get_full_alt_name (&name);
            print_info(_env, format!("Linking: {:#?} -> {:#?}",link,full_name));
            if _env.run_mode >= RunMode::Full {
                match symlink(full_name.clone(),link) {
                    Ok(()) => {}
                    Err(_why) => {
                        print_error(_env, format!("Error while linking: {:#?} -> {:#?}",link,full_name));
                        print_error(_env, format!("Reason: {}",_why));
                        return Err(Errors::Symlink);
                    }
                    
                }
            }

            print_info(_env, format!("Linking: {:#?} -> {:#?}",full_name,path));
            if _env.run_mode >= RunMode::Full {
                match symlink(path,full_name.clone()) {
                    Ok(()) => {}
                    Err(_why) => {
                        print_error(_env, format!("Error while linking: {:#?} -> {:#?}",full_name,path));
                        print_error(_env, format!("Reason: {}",_why));
                        return Err(Errors::Symlink);
                    }
                    
                }
            }
            return Ok(());
        }
        Records::Initscript{script} => {
            print_error(_env, format!("The initscript option is not yet implemented!"));
            return Err(Errors::Unimplemented);
        }
        _ => {
            print_error(_env, format!("This operation is not yet implemented!"));
            return Err(Errors::Unimplemented);

        }
    }
}

fn disable_record (_env: &Settings, rec: &Records) -> Result<(),Errors> {
    match rec {
        Records::File{link, name, path: _} => {
            print_info(_env, format!("Unlinking: {:#?}",link));

            if _env.run_mode >= RunMode::Full {
                let link_path = Path::new(link);
                if link_path.is_symlink() {
                    match fs::remove_file(link_path) {
                        Ok(_) => {}
                        Err(why) => {
                            //TODO: introduce --pedantic like switch and make this fatal
                            print_warning(_env, format!("Non fatal error while unlinking {}",link));
                            print_warning(_env, format!("Reason: {}", why));
                        }
                    }
                }
            }

            let full_name = _env.get_full_alt_name (&name);

            print_info(_env, format!("Unlinking: {:#?}",full_name));
            if _env.run_mode >= RunMode::Full {
                if full_name.is_symlink() {
                    match fs::remove_file(&full_name) {
                        Ok(_) => {}
                        Err(why) => {
                            //TODO: introduce --pedantic like switch and make this fatal
                            print_warning(_env, format!("Non fatal error while unlinking {:#?}",full_name));
                            print_warning(_env, format!("Reason: {}", why));
                        }
                    }
                }
            }
            return Ok(());
        }
        Records::Initscript{script} => {
            print_error(_env, format!("The initscript option is not yet implemented!"));
            return Err(Errors::Unimplemented);
        }
        _ => {
            print_error(_env, format!("This operation is not yet implemented!"));
            return Err(Errors::Unimplemented);
            
        }
    }    
}


fn print_help() {
    eprintln!("Usage: alternatives [OPTIONS] COMMAND");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  --install <link> <name> <path> <priority> [--follower <link> <name> <path>]...");
    eprintln!("                              Install an alternative");
    eprintln!("  --remove <name> <path>      Remove an alternative");
    eprintln!("  --export <name> <id> ...    Export alternatives as JSON");
    eprintln!("  --import <file>...          Install alternatives from JSON file(s)");
    eprintln!("  --remove-batch <file>...    Remove alternatives specified in JSON file(s)");
    eprintln!("  --install-export <link> <name> <path> <priority> [--follower ...]...");
    eprintln!("                              Convert install arguments to JSON (no DB changes)");
    eprintln!("  --help                      Show this help message");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --altdir <dir>              Alternatives directory (default: /tmp)");
    eprintln!("  --admindir <dir>            Admin/DB directory (default: /tmp)");
    eprintln!("  --verbose, --verbosity=<level>  Set verbosity (error/warning/info/debug)");
    eprintln!("  --dry-run                   Preview mode (default, no changes)");
    eprintln!("  --no-dry-run                Full mode (update DB and symlinks)");
    eprintln!("  --runmode=<mode>            Set run mode (preview/dbonly/full)");
    eprintln!();
    eprintln!("Install sub-options:");
    eprintln!("  --follower <link> <name> <path>  Add a follower to the alternative");
    eprintln!("  --initscript <script>       Associate an initscript (not yet implemented)");
    eprintln!("  --family <name>             Set alternative family (not yet implemented)");
}


fn print_message (_env: &Settings, mes: Message) {
    match mes {
           Message::Error{message} => {
                if _env.verbosity >= Verbosity::Error {
                    let mes: String = message.lines().map(|x| format!("Error: {}\n",x)).collect();
                    eprint!("{}",mes);
                }
            }
            Message::Warning{message} => {
                if _env.verbosity >= Verbosity::Warning {
                    let mes: String = message.lines().map(|x| format!("Warning: {}\n",x)).collect();
                    eprint!("{}",mes);
                }
            }
            Message::Info{message} => {
                if _env.verbosity >= Verbosity::Info {
                    let mes: String = message.lines().map(|x| format!("Info: {}\n",x)).collect();
                    eprint!("{}",mes);
                }
            }
            Message::Debug{message} => {
                if _env.verbosity >= Verbosity::Debug {
                    let mes: String = message.lines().map(|x| format!("Debug: {}\n",x)).collect();
                    eprint!("{}",mes);
                }
            }
        }
}
//wrapper functions for each level of verbosity
fn print_error (_env: &Settings, mes: String) {
    print_message (_env, Message::Error{message: mes});
}
fn print_warning (_env: &Settings, mes: String) {
    print_message (_env, Message::Warning{message: mes});
}
fn print_info (_env: &Settings, mes: String) {
    print_message (_env, Message::Info{message: mes});
}
fn print_debug (_env: &Settings, mes: String) {
    print_message (_env, Message::Debug{message: mes});
}

trait ToJson {
    fn sanitize_json (_s: &str) -> String {
        let mut rv = "".to_string();
        for c in _s.chars() {
            match c {
                '\\' | '"'  => { //TODO add the unicode control chars
                    rv.push('\\');
                    rv.push(c)
                }
                '\n' => {rv.push_str("\\n");}
                '\r' => {rv.push_str("\\r");}
                '\t' => {rv.push_str("\\t");}
                '\x08' => {rv.push_str("\\b");}
                '\x0C'  => {rv.push_str("\\f");}
                //TODO add the unicode control chars
                _ => {
                    rv.push(c)
                }
            }
        }
        rv
    }

    fn to_json(&self, _env:&Settings) -> String;
}

impl ToJson for Vec<Alternative> {
    fn to_json (&self, _env: &Settings) -> String {
        let mut content = "".to_string();
        content.push_str(&format!("[\n"));
        let mut comma = false;
        for alt in self {
            if comma {
                content.push_str(&format!("        ,\n")); //TODO -> put the comma on the previous line to make the json a bit prettier
            }
            content.push_str(alt.to_json(_env).as_str());
            comma = true;
        }
        content.push_str(&format!("]\n"));
        content
    }
    
}

impl ToJson for Records {
    fn to_json (&self, _env: &Settings) -> String {
        let mut rv = "".to_string();
        match &self {
            Records::File {link,name,path} => {
                rv.push_str(&format!("      {{\n"));
                rv.push_str(&format!("        \"File\": {{\n"));
                rv.push_str(&format!("          \"link\": \"{}\",\n", Self::sanitize_json(link))); 
                rv.push_str(&format!("          \"name\": \"{}\",\n", Self::sanitize_json(name)));
                rv.push_str(&format!("          \"path\": \"{}\"\n", Self::sanitize_json(path)));
                rv.push_str(&format!("        }}\n"));
                rv.push_str(&format!("      }}\n"));
            },
            Records::Initscript {script} => {
                rv.push_str(&format!("      {{\n"));
                rv.push_str(&format!("        \"Initscript\": {{\n"));
                rv.push_str(&format!("          \"script\": \"{}\"\n", Self::sanitize_json(script)));
                rv.push_str(&format!("        }}\n"));
                rv.push_str(&format!("      }}\n"));
            },
            _ => {},
        }
        rv
    }
}

impl ToJson for Alternative {
    fn to_json (&self, _env: &Settings) -> String {
        let mut rv = "".to_string();
        rv.push_str(&format!("  {{\n"));
        rv.push_str(&format!("    \"name\": \"{}\",\n", Self::sanitize_json(&self.name)));
        rv.push_str(&format!("    \"identifier\": \"{}\",\n", Self::sanitize_json(&self.identifier)));
        rv.push_str(&format!("    \"priority\": {},\n", self.priority));
        rv.push_str(&format!("    \"records\": [\n"));

        let mut comma = false;
        for rec in &self.records {
            if comma {
                rv.push_str(&format!("        ,\n")); //TODO -> put the comma on the previous line to make the json a bit prettier
            }
            rv.push_str(&rec.to_json(_env));
            comma = true;
        }

        
        rv.push_str(&format!("    ]\n"));
        rv.push_str(&format!("  }}\n"));
        rv

    }
}

// Check the db directory and get a list of all .json files there
// Currently unused function
/*fn list_db_files (_env: &Settings, path_name: PathBuf) -> Result<Vec<PathBuf>,Errors> {

    print_debug(_env,format!("Setting path to: {:#?}",path_name));

    let dir_path = Path::new(&path_name);
    let entries = match fs::read_dir(dir_path) {
        Ok(entries)  => {entries}
        Err(why) => {
            print_error(_env,format!("Error while reading the db directory: {:?}\n {:?}",dir_path,why));
            return Err(Errors::Unimplemented);
        }
    };

    // Filter out all entries with the wrong extension
    // TODO Depending on how we want to recover from possible errors, this might be simplified by using collect()
    // For now we try to handle each error individualy 
    let mut rv = Vec::new();
    for entry in entries {
       print_debug(_env,format!("Checking entry: {:#?}",entry));
       match entry {
           Ok(ref e) => {
               match e.path().extension() {
                   None => { //skip this entry
                           print_debug(_env,format!("Skipping entry: {:#?}",entry));
                   }
                   Some(ext) => {
                       print_debug(_env, format!("Extension: {:#?}",ext));
                       if ext == "json" {
                           print_debug(_env, format!("Adding path: {:#?}",e.path()));
                           rv.push(e.path());
                       }
                   }
               }
           }
           Err(why) => {
               print_error(_env, format!("{:#?}",why)); 
               return Err(Errors::Unimplemented);
           }
       } 
    }
    Ok(rv)
}*/

fn read_db_file (_env: &Settings, file_name: &PathBuf) -> Result<Vec<Alternative>,Errors> {
    let mut rv: Vec<Alternative>;
    let f = match read_to_string(file_name) {
        
        Ok(file) => {file}
        Err(e) => {
            match e.kind() {
                std::io::ErrorKind::NotFound => {
                    print_info(_env, format!("DB file not found: {:?}",file_name));
                    return Err(Errors::DBFileNotFound);
                }
                std::io::ErrorKind::PermissionDenied => {
                    print_error(_env, format!("File permissions error: {:?}",file_name));
                    return Err(Errors::DBPermissions);
                }
                _ => {
                    print_error(_env, format!("Unknown I/O error: {:?}",file_name));
                    return Err(Errors::Unknown);
                }
            }
        }

    };
    
    match serde_json::from_str(&f) {
        Err(why) => {
            print_error(_env, format!("{}",why));
            return Err(Errors::Json);
        }
        Ok(alts) => {rv = alts;}
    };
    for alt in rv.iter_mut() {
        alt.db_file = Some(file_name.clone());
    }

    print_debug(_env, format!("Read DB file:\n{:#?}",rv));
    Ok(rv)
}

fn remove_db_file(_env: &Settings, path: PathBuf) -> Result<(),Errors> {

    print_debug(_env, format!("Removing File:{:#?}",path));
    if _env.run_mode > RunMode::Preview {
        match fs::remove_file(path) {
            Ok(_) => {return Ok(())}
            _ => {return Err(Errors::Unimplemented)}
        }
    } else {
        print_info(_env, format!("Running in dry-run mode. The following file would have been removed: {:#?}",path));
        return Ok(());
    }
}

fn write_db_file(_env: &Settings, alts: &Vec<Alternative>) -> Result<(),Errors> {
    /*
     * The function expects that all the alternatives are to be written to the same file (determined by the first alternative in the list)
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

    
    print_info(_env, format!("Updating file {:#?}",file_name));

    let content = alts.to_json(&_env);
    if _env.run_mode > RunMode::Preview {
        let mut file = match File::create(&path) {
            Err(why) => {
                print_error(_env,format!("I/O error while creating db file: {:?}\n{:?}",path,why));
                return Err(Errors::DBFileError)
            },
            Ok(file) => file,
        };

        print_debug(_env, format!("Writing:\n{}",content));

        match file.write_all(content.as_bytes()) {
            Err(why) => {
                print_error(_env,format!("I/O error while writing to db file: {:?}\n{:?}",path,why));
                return Err(Errors::DBFileError)
            }
            Ok(_) => {}
        }
    } else {
        print_debug(_env, format!("Writing:\n{}",content));
    }

    Ok(())
}

#[derive(Clone,Debug)]
pub struct Settings {
    verbosity: Verbosity,
    run_mode: RunMode,
    admin_dir: String,
    alternatives_dir: String,
}

impl Default for Settings {
    fn default () -> Self {
        Settings {
            verbosity: Verbosity::Warning,
            run_mode: RunMode::Preview,
            admin_dir: "/tmp".to_string(),
            alternatives_dir: "/tmp".to_string(),
        }    
    }
}

impl Settings {

    fn get_full_alt_name (&self,alt: &str) -> PathBuf {
       return PathBuf::from(format!("{}/{}",self.alternatives_dir,alt)); 
       //return format!("{}/{}.json",self.admin_dir,alt); 
    }

    fn get_db_file_name (&self,alt: &str) -> PathBuf {
       return PathBuf::from(format!("{}/{}.json",self.admin_dir,alt)); 
       //return format!("{}/{}.json",self.admin_dir,alt); 
    }

    fn parse_args () -> Result<(Command, Settings),Errors> {
        //Just a wrapper function to simplify the parsing of the "--key value" values
        fn get_next_value (a: Option<String>) -> Result<String,Errors> {
            match a {
                Some(x) => {return Ok(x)}
                None =>  {
                    return Err(Errors::MissingArguments);
                }
            }
        }
        
        
        let mut args = env::args().skip(1); //Get the arguments, while skipping the name of the binary

        let rv_command;// = Command::Help;
        let mut rv_settings: Settings = Default::default();
        if args.len() == 0 {
            // Most of the times, this function expects to find a next argument and thus the Error type.
            // In some cases, the function is also used to check there are no extra unexpected arguments.
            // In those cases the Error Type can be a bit unintuitive (It says the exact opposite), since the first case is more common, and the dear reader will have to just accept it.
            return Err(Errors::MissingArguments); 
        }
        /*
         * Lets deal with the common options first, when done, i should be the index of the command
         */
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--debug" | "--verbosity=debug" => {rv_settings.verbosity = Verbosity::Debug}
                "--verbose" | "--verbosity=info"   => {rv_settings.verbosity = Verbosity::Info}
                "--warnings" | "--verbosity=warning"   => {rv_settings.verbosity = Verbosity::Warning}
                "--errors" | "--verbosity=error"   => {rv_settings.verbosity = Verbosity::Error}

                "--runmode=full" | "--no-dry-run" => {rv_settings.run_mode = RunMode::Full}
                "--runmode=dbonly" | "--partial-dry-run" => {rv_settings.run_mode = RunMode::DbOnly}
                "--runmode=preview" | "--dry-run" => {rv_settings.run_mode = RunMode::Preview}

                "--help" => {
                    rv_command = Command::Help;
                    return Ok((rv_command,rv_settings));
                }
                "--altdir" => {
                    rv_settings.alternatives_dir = get_next_value(args.next())?;
                }
                "--admindir" => {
                    rv_settings.admin_dir = get_next_value(args.next())?;
                }
                
                "--import" | "--install-batch" => {
                    let mut paths: Vec<PathBuf> = vec![];
                    while let Some(file) = args.next() {
                            let path = match PathBuf::from(file.clone()).canonicalize() {
                                Ok(p) => {p}
                                Err(why) => {
                                    print_error(&rv_settings,format!("Error while parsing --install-batch argument"));
                                    print_error(&rv_settings,format!("Argument: {}, Reason: {}",file,why));
                                    return Err(Errors::WrongArguments)
                                }
                            };
                            paths.push(path); 
                    }
                    if !paths.is_empty() {
                        let rv_command = Command::Import{paths: paths};
                        return Ok((rv_command,rv_settings));
                    } else {
                        print_error(&rv_settings, format!("Missing the file path argument"));
                        return Err(Errors::MissingArguments);
                    }
                }

                /*
                 * These two use the same syntax, but their function differs
                 * --install adds the alternative to the database and updates the links/scripts as needed
                 * --install-export only converts the input arguments to the json format and prints it to the stdout. The file system or the DB are not modified
                 */
                "--install" | "--install-export" => {
                    let mut alternative;// = Alternative::new();
                    // Check which variant are we actually using, before the 'i' gets modified
                    let install_export = arg.as_str() == "--install-export";

                    //there should be at least 4 more arguments (the 4th one is the priority)
                    let link = get_next_value(args.next())?;
                    let name = get_next_value(args.next())?;
                    let path = get_next_value(args.next())?;
                    //And this one should be an int
                    let prio: i32 = if let Some(prio) = args.next() {
                        match prio.parse() {
                            Ok(p) => {p}
                            Err(_why) => {
                                print_error(&rv_settings,format!("Could not convert the priority to number: {}",_why));
                                return Err(Errors::WrongArguments);
                            }
                        }
                    } else {
                        return Err(Errors::MissingArguments);
                    };
                    
                    alternative = Alternative::new(
                        name.clone(),
                        path.clone(),
                        prio,
                        Some(PathBuf::from(format!("{}/{}.json",rv_settings.admin_dir,name))), //TODO use a function instead
                    );

                    alternative.records.push(Records::File {                    
                        link: link.clone(),
                        name: name.clone(),
                        path: path.clone(),
                    });

                    //Now, check for the optional arguments (followers, etc.)
                    while let Some(optional) = args.next() {
                        match optional.as_str() {
                            "--follower" | "--slave" => {
                                let link = get_next_value(args.next())?;
                                let name = get_next_value(args.next())?;
                                let path = get_next_value(args.next())?;

                                alternative.records.push(Records::File {                    
                                    link: link.clone(),
                                    name: name.clone(),
                                    path: path.clone(),
                                });

                            }
                            "--initscript"  => {
                                //TODO: currently unimplemented
                                let _script = get_next_value(args.next())?;
                                alternative.records.push ( Records::Initscript {
                                  script: _script.clone()  
                                });
                                
                                print_warning(&rv_settings, format!("The --initscript option is currently unimplemented, (DB will be updated, but the script will be ignored)!"));
                            }
                            "--family"  => {
                                //TODO: currently unimplemented
                                let _family = get_next_value(args.next())?;
                                print_warning(&rv_settings, format!("The --family option is currently unimplemented, (Ignoring it)!"));
                            }

                            _ => {
                                print_error(&rv_settings,format!("Unexpected argument: {}",optional));
                                return Err(Errors::WrongArguments);
                            }
                            
                        }
                    }
                    

                    rv_command = if install_export
                    {
                        Command::InstallExport{alternative: alternative}
                    }
                    else {
                        Command::Install{alternative: alternative}
                    };

                    return Ok((rv_command,rv_settings));

                }
                "--remove" | "--uninstall" => {
                    let name = get_next_value(args.next())?;
                    let path = get_next_value(args.next())?;
                    let rv_command = Command::Remove{
                        name: name.clone(),
                        path: path.clone(),
                    };
                    match get_next_value(args.next()) { // There should be no more arguments
                        Ok(a) => {
                                print_error(&rv_settings,format!("Unexpected argument: {}",a));
                                return Err(Errors::WrongArguments)
                        }
                        Err(_) => {
                            return Ok((rv_command,rv_settings));
                        }

                    }
                }

                "--remove-batch" | "--uninstall-batch" => {
                    let mut paths: Vec<PathBuf> = vec![];
                    while let Some(path) = args.next() {
                        let path = match PathBuf::from(path.clone()).canonicalize() {
                            Ok(p) => {p}
                            Err(_why) => {
                                print_error(&rv_settings,format!("Error while parsing --uninstall-batch argument"));
                                print_error(&rv_settings,format!("Argument: {}, Reason: {}",path,_why));
                                return Err(Errors::WrongArguments)
                            }
                        };
                        paths.push(path); 
                    }
                    if !paths.is_empty() {
                        let rv_command = Command::UninstallBatch{paths: paths};
                        return Ok((rv_command,rv_settings));
                    } else {
                        print_error(&rv_settings, format!("Missing the file path argument(s)"));
                        return Err(Errors::MissingArguments);
                    }
                }
                
                "--export" => {
                    let mut alts: Vec<(String,String)> = vec![];
                    while let Some(name) = args.next() {
                        let path = get_next_value(args.next())?;
                            alts.push((name.clone(),path.clone()));
                    }

                    if !alts.is_empty() {
                        let rv_command = Command::Export{alternatives: alts};
                        return Ok((rv_command,rv_settings));
                    } else {
                        print_error(&rv_settings, format!("No alternatives were specified."));
                        return Err(Errors::MissingArguments);
                    }
                }

                _ => {
                    print_error(&rv_settings,format!("Unknown argument {}", arg));
                    return Err(Errors::WrongArguments);
                }
            }
        }
        return Err(Errors::InternalError);
    }   
}


fn main() -> ExitCode {
    
    let (command, _env) = match Settings::parse_args() {
        Ok ((c,e)) => {(c,e)}
        Err (why) => {
            print_help();
            let _env: Settings = Default::default();
            print_error(&_env,format!("{}",why.error_messages()));
            return ExitCode::from(why as u8);
        }
    };

    //These two dir checks are based on the behaviour of the original tool
    // create the admin dir if it does not exist
    if !Path::new(&_env.admin_dir).is_dir() {
        //Try creating a new directory. The lack of recursive trait should not overwrite existing files
        print_info(&_env,format!("Trying to create the admin directory."));
        match fs::create_dir(&_env.admin_dir) {
            Err(_why) => {
                print_error(&_env,format!("Could not create the admin directory."));
                print_error(&_env,format!("{}",_why));
                return ExitCode::from(Errors::AdminDirPermissions as u8)
            }
            _ => {}
        }
    }
    //Check for the existence of the alts dir (fatal error if not)
    if !Path::new(&_env.alternatives_dir).is_dir() {
        print_error(&_env,format!("Alternatives directory does not exist."));
        return ExitCode::from(Errors::AltDirPermissions as u8)
    }
    

    let rv: Result<(),Errors> = match command {
        Command::Help => {
            print_help();
            Ok(())
        }
        Command::Install{alternative} => {
            alternative.install(&_env)
        }
        Command::Remove{name, path} => {
            Alternative::uninstall(&_env,&name,&path)
        }
        Command::Export{alternatives} => {
            Alternative::export_alternatives(&_env,alternatives)
        }
        Command::Import{paths} => {
            paths.into_iter().map(|x| Alternative::dropin_import(&_env, x)).collect() 
        }
        Command::UninstallBatch{paths} => {
            paths.into_iter().map(|x| Alternative::dropin_remove(&_env, x)).collect() 
        }
        Command::InstallExport{alternative} => {
            let mut aux = vec![];
            aux.push(alternative);
            print!("{}",aux.to_json(&_env));
            Ok(())
        }


    };
    match rv {
        Ok(_) => {ExitCode::SUCCESS}
        Err(_why) => {
            print_error(&_env,format!("{}",_why.error_messages()));
            ExitCode::from(_why as u8)
        }
        
    }

}
