use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    process::exit,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local};
use rpassword::read_password;
use serde::{Deserialize, Deserializer, Serialize};

use serde::ser::{SerializeStruct, Serializer};

type CommandResult = Result<(), String>;

#[derive(Debug, Clone, Copy)]
struct SerializableInstant(Instant);
impl SerializableInstant {
    fn now() -> Self {
        Self(Instant::now())
    }
    fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }
}
impl Serialize for SerializableInstant {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(0)
    }
}
impl<'de> Deserialize<'de> for SerializableInstant {
    fn deserialize<D>(deserializer: D) -> Result<SerializableInstant, D::Error>
    where
        D: Deserializer<'de>,
    {
        i32::deserialize(deserializer)?;
        Ok(SerializableInstant(Instant::now()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Student {
    pub first: String,
    pub last: String,
    /// When popped, time spent in the queue is put here.
    pub queue_times: Vec<(Duration, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StaffMember {
    /// Checkin times.
    pub checkin_times: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueuedStudent {
    /// Time the student entered into the queue.
    pub entry_time: SerializableInstant,
    /// Key into the students [`HashMap`]
    pub net_id: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct QueueState {
    /// Roster of all students.
    pub students: HashMap<String, Student>,
    /// Staff members, key'd by netid.
    pub staff: HashMap<String, StaffMember>,
    /// The actual queue.
    pub queue: VecDeque<QueuedStudent>,
    /// Is the queue locked.
    pub locked: bool,
}
impl QueueState {
    fn authenticate(&self) -> CommandResult {
        print!("Enter password:");
        std::io::stdout().flush().unwrap();
        let password = read_password().unwrap();
        if password == "53rocks" {
            Ok(())
        } else {
            Err("Invalid password.".to_owned())
        }
    }

    pub fn save_backup(&self) {
        let Ok(mut file) = File::create("backup.txt") else {
            println!("Invalid file.");
            return;
        };

        let Ok(output) = serde_json::to_string(self) else {
            println!("Failed to serialize.");
            return;
        };

        let Ok(_) = file.write_all(output.as_bytes()) else {
            println!("Failed to write bytes.");
            return;
        };
    }

    pub fn load_backup(&mut self) {
        let Ok(contents) = std::fs::read_to_string("backup.txt") else {
            println!("Backup file does not exist.");
            return;
        };

        let Ok(new_self) = serde_json::from_str(&contents) else {
            println!("Failed to parse backup file.");
            return;
        };

        *self = new_self;

        println!("Loaded from backup.");
    }

    /// Staff log in.
    ///
    /// `checkin <netid>`
    pub fn checkin(&mut self, parts: &[String]) -> CommandResult {
        if parts.len() < 2 {
            return Err("Usage: \"checkin <netid>\".".to_owned());
        }

        self.authenticate()?;

        let Some(staff_member) = self.staff.get_mut(&parts[1]) else {
            return Err("Not a member of staff. Message James on slack.".to_owned());
        };

        staff_member.checkin_times.push(format!(
            "{}",
            chrono::offset::Local::now().format("%d/%m/%Y %H:%M")
        ));

        self.save_backup();

        println!("{} checked in.", parts[1]);
        Ok(())
    }
    /// Add a name to the queue.
    ///
    /// `add <netid>`
    pub fn add(&mut self, parts: &[String]) -> CommandResult {
        if parts.len() < 2 {
            return Err("Usage: \"add <netid>\".".to_owned());
        }

        if !self.students.contains_key(&parts[1]) {
            return Err(
                "Not a student. Contact course staff if you believe this is a mistake.".to_owned(),
            );
        };

        if self.locked {
            return Err("Queue is locked.".to_owned());
        }

        if let Some((i, _)) = self
            .queue
            .iter()
            .enumerate()
            .find(|(_, entry)| entry.net_id == parts[1])
        {
            return Err(format!("Already in the queue, position: {}", i));
        }

        self.queue.push_back(QueuedStudent {
            entry_time: SerializableInstant::now(),
            net_id: parts[1].clone(),
        });

        println!("Added to queue in position {}", self.queue.len());
        self.save_backup();
        Ok(())
    }
    /// Remove someone from the queue. Optionally record who popped them.
    ///
    /// `pop`
    pub fn pop(&mut self) -> CommandResult {
        self.authenticate()?;

        let Some(student) = self.queue.pop_front() else {
            return Err("Queue is empty.".to_owned());
        };
        let time_in_queue = student.entry_time.elapsed();

        let student = self.students.get_mut(&student.net_id).unwrap();
        student.queue_times.push((
            time_in_queue,
            format!("{}", chrono::offset::Local::now().format("%d/%m/%Y %H:%M")),
        ));

        println!(
            "Popped: \"{} {}\" after {:?} in queue.",
            student.first, student.last, time_in_queue
        );

        self.save_backup();

        Ok(())
    }
    /// View's the queue.
    ///
    /// `view`
    pub fn view(&mut self) -> CommandResult {
        if self.queue.is_empty() {
            println!("Queue is empty.");
            return Ok(());
        }
        if self.locked {
            println!("QUEUE IS LOCKED!");
        }
        for (i, student) in self.queue.iter().enumerate() {
            let time_in_queue = student.entry_time.elapsed();
            let student = self.students.get(&student.net_id).unwrap();
            println!(
                "{}: {} {} for {:?}",
                i, student.first, student.last, time_in_queue
            );
        }
        Ok(())
    }
    /// Clears the screen.
    ///
    /// `clear`
    pub fn clear(&mut self) -> CommandResult {
        self.authenticate()?;
        if clearscreen::clear().is_err() {
            return Err("Failed to clear screen.".to_owned());
        }
        Ok(())
    }
    /// Dumps the stats to a file.
    ///
    /// `stats <filename>`
    pub fn stats(&mut self, parts: &[String]) -> CommandResult {
        if parts.len() < 2 {
            return Err("Usage: \"stats <filename>\".".to_owned());
        }
        self.authenticate()?;

        let Ok(mut file) = File::create(&parts[1]) else {
            return Err("Invalid file.".to_owned());
        };

        let Ok(output) = serde_json::to_string_pretty(self) else {
            return Err("Failed to serialize.".to_owned());
        };

        let Ok(_) = file.write_all(output.as_bytes()) else {
            return Err("Failed to write bytes.".to_owned());
        };

        println!("Stats saved.");
        Ok(())
    }
    /// Resets the stats, presumably after dumping them using the above command.
    ///
    /// `reset`
    pub fn reset(&mut self) -> CommandResult {
        self.authenticate()?;
        for student in self.students.values_mut() {
            student.queue_times.clear();
        }
        self.queue.clear();
        self.locked = false;
        Ok(())
    }
    /// Locks the queue.
    ///
    /// `lock`
    pub fn lock(&mut self) -> CommandResult {
        self.authenticate()?;
        self.locked = true;
        println!("Queue is locked.");
        Ok(())
    }
    /// Unlocks the queue.
    ///
    /// `unlock`
    pub fn unlock(&mut self) -> CommandResult {
        self.authenticate()?;
        self.locked = false;
        println!("Queue is unlocked.");
        Ok(())
    }
    /// Prints help.
    ///
    /// `help`
    pub fn help(&mut self) -> CommandResult {
        println!("\"add <netid>\" - adds the specified netid to the queue.");
        println!("\"view\" - views the queue.");
        Ok(())
    }
    /// Exit the queue, saves the global state before doing so.
    ///
    /// `quit`
    pub fn quit(&mut self) -> CommandResult {
        self.authenticate()?;
        self.save_backup();
        exit(0);
    }
    /// Load the global state from a file.
    ///
    /// `load <filename>`
    pub fn load(&mut self, parts: &[String]) -> CommandResult {
        if parts.len() < 2 {
            return Err("Usage: \"save <filename>\".".to_owned());
        }
        self.authenticate()?;

        let Ok(contents) = std::fs::read_to_string(&parts[1]) else {
            return Err("Invalid file.".to_owned());
        };

        let Ok(new_self) = serde_json::from_str(&contents) else {
            return Err("Failed to parse file.".to_owned());
        };

        *self = new_self;

        println!("Loaded from file.");

        Ok(())
    }
    /// Save the global state forcefully.
    ///
    /// `save <filename>`
    pub fn save(&mut self, parts: &[String]) -> CommandResult {
        if parts.len() < 2 {
            return Err("Usage: \"save <filename>\".".to_owned());
        }
        self.authenticate()?;

        let Ok(mut file) = File::create(&parts[1]) else {
            return Err("Invalid file.".to_owned());
        };

        let Ok(output) = serde_json::to_string(self) else {
            return Err("Failed to serialize.".to_owned());
        };

        let Ok(_) = file.write_all(output.as_bytes()) else {
            return Err("Failed to write bytes.".to_owned());
        };

        println!("State saved.");
        return Ok(());
    }

    /// Add a staff member.
    ///
    /// `add_staff <netid>`
    pub fn add_staff(&mut self, parts: &[String]) -> CommandResult {
        if parts.len() < 2 {
            return Err("Usage: \"add_staff <netid>\".".to_owned());
        }
        self.authenticate()?;
        if self.staff.contains_key(&parts[1]) {
            return Err(format!("{} is already a staff member.", parts[1]));
        }
        self.staff.insert(
            parts[1].clone(),
            StaffMember {
                checkin_times: Vec::default(),
            },
        );
        println!("Staff member {} added.", parts[1]);
        self.save_backup();
        Ok(())
    }

    /// Load a roster. Overwrites the current one.
    ///
    /// `load_roster <path_to_file>`
    pub fn load_roster(&mut self, parts: &[String]) -> CommandResult {
        if parts.len() < 1 {
            return Err("Usage: \"load_roster <path_to_file>\".".to_owned());
        }
        println!("parts: {:?}", parts);
        self.authenticate()?;

        let Ok(file) = OpenOptions::new().read(true).open(parts[1].clone()) else {
            return Err("Invalid file1.".to_owned());
        };

        self.students.clear();

        for (i, line) in BufReader::new(file).lines().flatten().enumerate() {
            let line_parts = line
                .split(",")
                .map(|s| s.to_owned())
                .collect::<Vec<String>>();

            if 4 != line_parts.len() {
                return Err(format!("Err on line {}:{} ", i, line));
            }

            let netid = line_parts[2].to_lowercase();
            if !self.students.contains_key(&netid) {
                self.students.insert(
                    netid,
                    Student {
                        first: line_parts[1].to_lowercase(),
                        last: line_parts[0].to_lowercase(),
                        queue_times: Vec::default(),
                    },
                );
            }
        }

        println!("Imported {} students.", self.students.len());
        self.save_backup();

        Ok(())
    }

    pub fn process_command(&mut self, command: &str) -> CommandResult {
        let parts = command
            .split_ascii_whitespace()
            .map(|s| s.to_owned())
            .collect::<Vec<String>>();
        if parts.is_empty() {
            return Err("Command is empty.".to_owned());
        }

        match parts[0].to_lowercase().as_str() {
            "checkin" => self.checkin(&parts),
            "add" => self.add(&parts),
            "pop" => self.pop(),
            "view" => self.view(),
            "clear" => self.clear(),
            "stats" => self.stats(&parts),
            "reset" => self.reset(),
            "lock" => self.lock(),
            "unlock" => self.unlock(),
            "help" => self.help(),
            "quit" => self.quit(),
            "load" => self.load(&parts),
            "save" => self.save(&parts),
            "add_staff" => self.add_staff(&parts),
            "load_roster" => self.load_roster(&parts),
            _ => Err("Unknown command.".to_owned()),
        }
    }
}

fn main() {
    // let vec = vec![(Duration::default(), "One".to_owned()),(Duration::default(), "Two".to_owned()),
    // (Duration::default(), "Three".to_owned()),(Duration::default(), "Four".to_owned()),];

    // println!("{}", serde_json::to_string_pretty(&vec).unwrap());

    // panic!();

    let mut queue = QueueState::default();
    queue.load_backup();
    let mut buffer = String::new();
    loop {
        std::io::stdin().read_line(&mut buffer).expect("Hmmmmm");
        if let Err(err) = queue.process_command(&buffer) {
            println!("Error: {}", err);
        }
        buffer.clear();
    }
}
