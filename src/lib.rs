/*
 * This file contains the definitions for the simulation types
 */

use core::panic;
use std::collections::HashMap;
use std::{io::Read};
// Global Imports
use std::error::Error;
use std::fs::File;
use std::path::PathBuf;
use std::vec::Vec;

use chrono::{DateTime, Utc};

use regex::Regex;

use tracing::{debug, error, warn};

// Local Imports

/* #### Enums #### */

#[derive(Debug, Eq, PartialEq)]
pub enum Mode {
    Transient,
    FFT,
    AC,
    DC,
    Noise,
    OperatingPoint,
}

#[derive(Debug, Eq, PartialEq)]
pub enum FileType {
    Binary,
    ASCII,
}

#[derive(Debug, Eq, PartialEq)]
pub enum DataType {
    Float32,
    Float64,
    Complex128,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Encoding {
    UTF8,
    UTF16,
    UTF32,
    ASCII,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Flags {
    Stepped,
    Real,
    Double,
}

#[derive(Debug, Eq, PartialEq)]
pub enum VariableClass {
    Voltage,
    Current,
    Frequency,
    Unknown,
}

/* #### Structs #### */

#[derive(Debug)]
pub struct SteppedVariable {
    class: VariableClass,
    name: String,
}

#[derive(Debug, Clone)]
pub struct Value {
    real: f64,
    imaginary: f64,
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.real == other.real && self.imaginary == other.imaginary
    }
}

#[derive(Debug)]
pub struct SimulationStats {
    variables: u32,
    points: u32,
    steps: u16,
    step_size: u32
}

#[derive(Debug)]
pub struct SteppedSimulation {
    path: PathBuf,
    encoding: Encoding,
    mode: Mode,
    flags: Vec<Flags>,
    date: DateTime<Utc>,
    stats: SimulationStats,
    variables: Vec<SteppedVariable>,
    data: HashMap<String, Vec<Vec<Value>>>,
}

/* #### Implementations #### */

impl SteppedSimulation {
    pub fn new(path: PathBuf) -> Self {
        return SteppedSimulation {
            path,
            encoding: Encoding::UTF8,
            mode: Mode::Transient,
            flags: Vec::new(),
            date: Utc::now(),
            stats: SimulationStats {
                variables: 0,
                points: 0,
                steps: 0,
                step_size: 0,
            },
            variables: Vec::new(),
            data: HashMap::new(),
        };
    }

    pub fn reload(&mut self) -> Result<(), Box<dyn Error>> {
        self.parse()?;
        Ok(())
    }

    fn parse(&mut self) -> Result<(), Box<dyn Error>> {

        /* #### File Checks #### */
        if !self.path.exists() {
            error!("The specified file does not exist: {:?}", self.path);
            Err("File does not exist")?;
        }

        if !self.path.is_file() {
            error!("The specified path is not a file: {:?}", self.path);
            Err("The specified path is not a file.")?;
        }

        if !self.path.extension().unwrap().eq("raw") {
            error!("The specified path is not a '.raw' file: {:?}", self.path);
            Err("The specified path is not a '.raw' file.")?;
        }

        /* #### Read File Binary Contents #### */

        let mut file = File::open(&self.path)?;

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        /* #### Parse Header #### */

        let mut decoded = false;
        let mut data = String::new();

        // Try UTF8 Encoding
        if !decoded {
            let local_buffer = buffer.clone();
            data = String::from_utf8_lossy(local_buffer.as_slice()).to_string();

            if data.contains("Values") || data.contains("Binary") {
                decoded = true;
                self.encoding = Encoding::UTF8;
            }
        }

        // Try UTF16 Encoding
        if !decoded {
            let local_buffer: Vec<u16> = buffer
                .chunks_exact(2)
                .into_iter()
                .map(|a| u16::from_le_bytes([a[0], a[1]]))
                .collect();

            data = String::from_utf16_lossy(local_buffer.as_slice()).to_string();

            if data.contains("Values") || data.contains("Binary") {
                decoded = true;
                self.encoding = Encoding::UTF16
            }
        }

        if !decoded {
            panic!("Could not decode file.");
        }

        // Split Header & Binary
        let substring = "Binary:\n";
        let index = data.find(substring).unwrap();
        let header_length = match self.encoding {
            Encoding::UTF8 => index + substring.len(),
            Encoding::UTF16 => (index + substring.len()) * 2,
            Encoding::UTF32 => (index + substring.len()) * 4,
            Encoding::ASCII => index + substring.len(),
        };

        buffer.drain(0..header_length);
        debug!(
            "Binary Size: {:.2}%",
            buffer.len() as f32 / data.len() as f32 * 100.0
        );

        let header = data.split_at(index + substring.len()).0;
        let mut values: HashMap<String, String> = HashMap::new();
        let re_text =
            Regex::new(r"(?:^|\n)([a-zA-Z .]*[a-zA-Z]+):((?:.+)|(?:(?:.|\n)+(?:Binary:)))")
                .unwrap();
        for cap in re_text.captures_iter(header) {
            values.insert(cap[1].to_string(), cap[2].to_string());
        }

        /* #### Parse Binary Data #### */

        // Load Values
        for (key, value) in values.iter() {
            match key.as_str() {
                "Title" => {}
                "Date" => {
                    self.date = match dateparser::parse(&value) {
                        Ok(date) => date,
                        Err(_) => Utc::now(),
                    };
                }
                "Plotname" => match value.as_str() {
                    "Transient Analysis" => self.mode = Mode::Transient,
                    "AC Analysis" => self.mode = Mode::AC,
                    "DC Analysis" => self.mode = Mode::DC,
                    "Noise Analysis" => self.mode = Mode::Noise,
                    "Operating Point" => self.mode = Mode::OperatingPoint,
                    "FFT" => self.mode = Mode::OperatingPoint,
                    _ => {}
                },
                "Flags" => match value.as_str() {
                    "stepped" => self.flags.push(Flags::Stepped),
                    "real" => self.flags.push(Flags::Real),
                    "double" => self.flags.push(Flags::Double),
                    _ => {}
                },
                "No. Points" => self.stats.points = value.trim().parse::<u32>()?,
                "No. Variables" => self.stats.variables = value.trim().parse::<u32>()?,
                "Variables" => {
                    let re = Regex::new(r"\s*(\d+)\s*([VIx]+\((?:.|:|\d)+\))\s*(\w+)\n").unwrap();
                    for cap in re.captures_iter(value) {
                        self.variables.push(SteppedVariable {
                            class: match &cap[3] {
                                "V" => VariableClass::Voltage,
                                "I" => VariableClass::Current,
                                _ => VariableClass::Unknown,
                            },
                            name: cap[2].to_string(),
                        });
                    }
                }
                "Command" => {}
                "Backannotation" => {}
                "Offset" => {}
                _ => {
                    warn!("Unknown LTSPICE Simulation Key: {}", key);
                }
            }
        }

        /* #### Binary Parsing #### */

        let mut x_type: DataType = DataType::Float64;
        let mut y_type: DataType = DataType::Float32;

        if self.flags.contains(&Flags::Double) {
            y_type = DataType::Float64;
        }

        if self.mode == Mode::AC || self.mode == Mode::FFT {
            x_type = DataType::Complex128;
            y_type = DataType::Complex128;
        }

        // Compute Data Lengths
        let y_size = match y_type {
            DataType::Float32 => 4,
            DataType::Float64 => 8,
            DataType::Complex128 => 16,
        };

        let x_size = match x_type {
            DataType::Float32 => 4,
            DataType::Float64 => 8,
            DataType::Complex128 => 16,
        };

        let y_length = self.stats.points * (self.stats.variables - 1) * y_size;
        let x_length = self.stats.points * x_size;

        let expected_length = x_length + y_length;

        if expected_length != buffer.len() as u32 {
            error!("There is a mismatch between the expected and actual SPICE data length.");
            error!("It is possible that this library is not yet able to handle this type of file.");
            error!("Please contact the library author.");
            Err("Mismatch between expected and actual SPICE data length.")?;
        }

        // Parse Buffer
        self.data.insert("x".to_string(), Vec::new());
        self.stats.step_size = expected_length;
        let mut iterator = buffer.into_iter();
        let mut x_buffer: Vec<Value> = Vec::new();
        while iterator.len() > 0 {

            // X Data
            let x_data = iterator.by_ref().take(x_size as usize).collect::<Vec<u8>>();

            // Read Real & Imaginary Parts
            let x_real = match x_type {
                DataType::Float32 => f32::from_ne_bytes(x_data.clone().try_into().unwrap()) as f64,
                DataType::Float64 => f64::from_ne_bytes(x_data.clone().try_into().unwrap()),
                DataType::Complex128 => f64::from_ne_bytes(x_data.clone().try_into().unwrap()),
            };
            let x_imaginary = match x_type {
                DataType::Float32 => 0.0,
                DataType::Float64 => 0.0,
                DataType::Complex128 => f64::from_ne_bytes(x_data.clone().try_into().unwrap()),
            };

            // Create The Value Object
            let x_value = Value {
                real: x_real,
                imaginary: x_imaginary,
            };

            // If we get the same value twice, we know we have a new step
            // In this case, we have to rotate the data vector
            if x_buffer.len() > 0 && x_buffer.first().unwrap().clone() == x_value {
                self.stats.step_size = x_buffer.len() as u32;
                self.stats.steps = (self.stats.points / x_buffer.len() as u32) as u16;
                self.data.get_mut("x").unwrap().push(x_buffer.clone());
                x_buffer.clear();
            }

            x_buffer.push(x_value);

            // After an X datapoint, the following bytes represent the different variables of the simulation.
            // We read them one by one and store them in the data HashMap.
            for variable in self.variables.iter() {

                // Create HashMap if it doesn't exist
                if self.data.get(&variable.name).is_none() {
                    self.data.insert(variable.name.clone(), Vec::new());
                }

                // Load the step vector
                let step_vector = self.data.get_mut(&variable.name).unwrap();

                // Create a new step vector if the current one is full or non-existent
                if step_vector.len() == 0 || self.stats.step_size == step_vector.last().unwrap().len() as u32 {
                    step_vector.push(Vec::new());
                }

                let vector = step_vector.last_mut().unwrap();

                // Y Data
                let y_data = iterator.by_ref().take(y_size as usize).collect::<Vec<u8>>();

                // Read the Real & Imaginary Parts
                let y_real = match y_type {
                    DataType::Float32 => {
                        f32::from_ne_bytes(y_data.clone().try_into().unwrap()) as f64
                    }
                    DataType::Float64 => f64::from_ne_bytes(y_data.clone().try_into().unwrap()),
                    DataType::Complex128 => f64::from_ne_bytes(y_data.clone().try_into().unwrap()),
                };
                let y_imaginary = match y_type {
                    DataType::Float32 => 0.0,
                    DataType::Float64 => 0.0,
                    DataType::Complex128 => f64::from_ne_bytes(y_data.clone().try_into().unwrap()),
                };

                // Create The Value Object
                let y_value = Value {
                    real: y_real,
                    imaginary: y_imaginary,
                };

                vector.push(y_value);

            }
        }

        // Load The Last X Data
        // This is necessary because the last step is not detected by the loop above
        self.data.get_mut("x").unwrap().push(x_buffer.clone());

        debug!("Loaded {} Variables.", self.data.len());
        debug!("Detected {} Steps.", self.stats.steps);
        debug!("Loaded {} Steps.", self.data.get("x").unwrap().len());
        debug!(
            "Loaded {} Values Per Step.",
            self.data.get("V(v_in)").unwrap().last().unwrap().len()
        );

        Ok(())
    }

    /* #### Data Interfaces #### */

    /// Returns a reference to the loaded variable, for the specified step.
    /// Returns None if no variable with the specified name exist.
    /// If no step is specified, the first step is returned.
    pub fn get(&self, name: &str, step: Option<u16>) -> Option<&Vec<Value>> {

        let step = match step {
            Some(step) => step,
            None => 0,
        };

        let data = match self.data.get(name) {
            Some(data) => data,
            None => return None,
        };

        return match data.get(step as usize) {
            Some(data) => Some(data),
            None => None,
        };

    }

    // Returns a reference to the loaded X data.
    pub fn get_x(&self) -> Option<&Vec<Value>> {
        return self.get("x", None);
    }

    // Returns a reference to the simulation steps.
    pub fn get_stats(&self) -> &SimulationStats {
        return &self.stats;
    }

    // Returns the loaded variables
    pub fn get_variables(&self) -> &Vec<SteppedVariable> {
        return &self.variables;
    }

}
