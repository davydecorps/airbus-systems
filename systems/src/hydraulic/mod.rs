use std::{borrow::Borrow, cmp::Ordering, fmt::Pointer};
use std::f64::consts;
use std::time::Duration;

//use uom::{si::{area::square_meter, f64::*, force::newton, length::foot, length::meter, mass_density::kilogram_per_cubic_meter, pressure::atmosphere, pressure::pascal, pressure::psi, ratio::percent, thermodynamic_temperature::{self, degree_celsius}, time::second, velocity::knot, volume::cubic_inch, volume::gallon, volume::liter, volume_rate::cubic_meter_per_second, volume_rate::{VolumeRate, gallon_per_second}}, typenum::private::IsLessOrEqualPrivate};
//use uom::si::f64::*;
use uom::{si::{acceleration::galileo, area::square_meter, f64::*, force::newton, length::foot, length::meter, mass_density::kilogram_per_cubic_meter, pressure::atmosphere, pressure::pascal, pressure::psi, ratio::percent, thermodynamic_temperature::{self, degree_celsius}, time::second, velocity::knot, volume::cubic_inch, volume::gallon, volume::liter, volume_rate::cubic_meter_per_second, volume_rate::gallon_per_second}, typenum::private::IsLessOrEqualPrivate};

use crate::{
    engine::Engine,
    simulation::UpdateContext,
};

// //Interpolate values_map_y at point value_at_point in breakpoints break_points_x
fn interpolation(xs: &[f64], ys: &[f64], intermediate_x: f64) -> f64 {
    debug_assert!(xs.len() == ys.len());
    debug_assert!(xs.len() >= 2);
    debug_assert!(ys.len() >= 2);
    // The function also assumes xs are ordered from small to large. Consider adding a debug_assert! for that as well.

    if intermediate_x <= xs[0] {
        *ys.first().unwrap()
    } else if intermediate_x >= xs[xs.len()-1] {
        *ys.last().unwrap()
    } else {
        let mut idx:usize =1;

        while idx < xs.len()-1 {
            if intermediate_x < xs[idx] {
               break;
            }
            idx += 1;
        }

        ys[idx-1] + (intermediate_x - xs[idx-1]) / (xs[idx] - xs[idx-1]) * (ys[idx] - ys[idx-1])
    }
}

// TODO:
// - Priority valve
// - Engine fire shutoff valve
// - Leak measurement valve
// - RAT pump implementation
// - Connecting electric pumps to electric sources
// - Connecting RAT pump/blue loop to emergency generator
// - Actuators
// - Bleed air sources for reservoir/line anti-cavitation

////////////////////////////////////////////////////////////////////////////////
// DATA & REFERENCES
////////////////////////////////////////////////////////////////////////////////
///
/// On A320, the reservoir level variation can, depending on the system,
/// decrease in flight by about 3.5 l (G RSVR), 4 l (Y RSVR) and 0.5 l (B RSVR)
///
/// Each MLG door open (2 total) uses 0.25 liters each of green hyd fluid
/// Each cargo door open (3 total) uses 0.2 liters each of yellow hyd fluid
///
/// Reservoirs
/// ------------------------------------------
/// Normal Qty:
/// -----------
/// Blue: 6.5L (1.7 US Gal)
/// Yellow: 12.5L (3.3 US Gal)
/// Green: 14.5L (3.8 US Gal)
///
/// Loops
/// ------------------------------------------
/// Max loop volume - green: 100L 26.41gal including reservoir
/// Max loop volume - yellow: 75L 19.81gal including reservoir
/// Max loop volume - blue: 50L 15.85gal including reservoir
///
/// EDP (Eaton PV3-240-10C/D/F (F is neo)):
/// ------------------------------------------
/// 37.5 GPM max (100% N2)
/// 3750 RPM
/// 3000 PSI
/// Displacement: 2.40 in3/rev
///
///
/// Electric Pump (Eaton MPEV3-032-EA2 (neo) MPEV-032-15 (ceo)):
/// ------------------------------------------
/// Uses 115/200 VAC, 400HZ electric motor
/// 8.45 GPM max
/// 7600 RPM at full displacement, 8000 RPM at no displacement
/// 3000 PSI
/// Displacement: 0.263 in3/rev
///
///
/// PTU (Eaton Vickers MPHV3-115-1C):
/// ------------------------------------------
/// 2987 PSI
///
/// Yellow to Green
/// ---------------
/// 34 GPM (130 L/min) from Yellow system
/// 24 GPM (90 L/min) to Green system
/// Maintains constant pressure near 3000PSI in green
///
/// Green to Yellow
/// ---------------
/// 16 GPM (60 L/min) from Green system
/// 13 GPM (50 L/min) to Yellow system
/// Maintains constant pressure near 3000PSI in yellow
///
///
/// RAT PUMP (Eaton PV3-115):
/// ------------------------------------------
/// Max displacement: 1.15 in3/rev, 18.85 mL/rev
/// Normal speed: 6,600 RPM
/// Max. Ov. Speed: 8,250 RPM
/// Theoretical Flow at normal speed: 32.86 gpm, 124.4 l/m
///
///
/// Equations:
/// ------------------------------------------
/// Flow (Q), gpm:  Q = (in3/rev * rpm) / 231
/// Velocity (V), ft/s: V = (0.3208 * flow rate, gpm) / internal area, sq in
/// Force (F), lbs: F = density * area * velocity^2
/// Pressure (P), PSI: P = force / area
/// PressureDelta = VolumeDelta / Total_uncompressed_volume * Fluid_Bulk_Modulus
///
///
/// Hydraulic Fluid: EXXON HyJet IV
/// ------------------------------------------
/// Kinematic viscosity at 40C: 10.55 mm^2 s^-1, +/- 20%
/// Density at 25C: 996 kg m^-3
///
/// Hydraulic Line (HP) inner diameter
/// ------------------------------------------
/// Currently unknown. Estimating to be 7.5mm for now?
///
///
/// Actuator Force Simvars
/// -------------------------------------------
/// ACCELERATION BODY X (relative to aircraft, "east/west", Feet per second squared)
/// ACCELERATION BODY Y (relative to aircraft, vertical, Feet per second squared)
/// ACCELERATION BODY Z (relative to aircraft, "north/south", Feet per second squared)
/// ROTATION VELOCITY BODY X (feet per second)
/// ROTATION VELOCITY BODY Y (feet per second)
/// ROTATION VELOCITY BODY Z (feet per second)
/// VELOCITY BODY X (feet per second)
/// VELOCITY BODY Y (feet per second)
/// VELOCITY BODY Z (feet per second)
/// WING FLEX PCT (:1 for left, :2 for right; settable) (percent over 100)
///

////////////////////////////////////////////////////////////////////////////////
// ENUMERATIONS
////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ActuatorType {
    Aileron,
    BrakesNormal,
    BrakesAlternate,
    BrakesParking,
    CargoDoor,
    Elevator,
    EmergencyGenerator,
    EngReverser,
    Flaps,
    LandingGearNose,
    LandingGearMain,
    LandingGearDoorNose,
    LandingGearDoorMain,
    NoseWheelSteering,
    Rudder,
    Slat,
    Spoiler,
    Stabilizer,
    YawDamper,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LoopColor {
    Blue,
    Green,
    Yellow,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PtuState {
    Off,
    GreenToYellow,
    YellowToGreen,
}

////////////////////////////////////////////////////////////////////////////////
// TRAITS
////////////////////////////////////////////////////////////////////////////////

// Trait common to all hydraulic pumps
// Max gives maximum available volume at that time as if it is a variable displacement
// pump it can be adjusted by pump regulation
// Min will give minimum volume that will be outputed no matter what. example if there is a minimal displacement or
// a fixed displacement (ie. elec pump)
pub trait PressureSource {
    fn get_delta_vol_max(&self) -> Volume;
    fn get_delta_vol_min(&self) -> Volume;
}

////////////////////////////////////////////////////////////////////////////////
// LOOP DEFINITION - INCLUDES RESERVOIR AND ACCUMULATOR
////////////////////////////////////////////////////////////////////////////////

//Implements fluid structure.
//TODO update method that can update physic constants from given temperature
//This would change pressure response to volume
pub struct HydFluid {
    //temp : thermodynamic_temperature,
    current_bulk : Pressure,
}

impl HydFluid {
    pub fn new ( bulk : Pressure) -> HydFluid {
        HydFluid{
            //temp:temp,
            current_bulk:bulk,
        }
    }

    pub fn get_bulk_mod (&self) -> Pressure {
        return self.current_bulk;
    }
}

//Power Transfer Unit
//TODO enhance simulation with RPM and variable displacement on one side?
pub struct Ptu {
    isEnabled : bool,
    isActiveRight : bool,
    isActiveLeft : bool,
    flow_to_right : VolumeRate,
    flow_to_left : VolumeRate,
    last_flow : VolumeRate,
}

impl Ptu {
    //Low pass filter to handle flow dynamic: avoids instantaneous flow transient,
    // simulating RPM dynamic of PTU
    const FLOW_DYNAMIC_LOW_PASS_LEFT_SIDE : f64 = 0.1;
    const FLOW_DYNAMIC_LOW_PASS_RIGHT_SIDE : f64 = 0.1;

    //Part of the max total pump capacity PTU model is allowed to take. Set to 1 all capacity used
    // set to 0.5 PTU will only use half of the flow that all pumps are able to generate
    const AGRESSIVENESS_FACTOR : f64 = 0.6;

    pub fn new() -> Ptu {
        Ptu{
            isEnabled : false,
            isActiveRight : false,
            isActiveLeft : false,
            flow_to_right : VolumeRate::new::<gallon_per_second>(0.0),
            flow_to_left : VolumeRate::new::<gallon_per_second>(0.0),
            last_flow : VolumeRate::new::<gallon_per_second>(0.0),
        }


    }

    pub fn get_flow(&self) -> VolumeRate {
        self.last_flow
    }

    pub fn get_is_active(&self) -> bool {
        self.isActiveRight || self.isActiveLeft
    }

    pub fn get_is_active_left_to_right(&self) -> bool {
        self.isActiveLeft
    }

    pub fn get_is_active_right_to_left(&self) -> bool {
        self.isActiveRight
    }

    pub fn update(&mut self,loopLeft : &HydLoop, loopRight: &HydLoop){
        if self.isEnabled {
            let deltaP=loopLeft.get_pressure() - loopRight.get_pressure();

            //TODO: use maped characteristics for PTU?
            //TODO Use variable displacement available on one side?
            //TODO Handle RPM of ptu so transient are bit slower?
            //TODO Handle it as a min/max flow producer using PressureSource trait?
            if self.isActiveLeft || (!self.isActiveRight && deltaP.get::<psi>()  > 500.0) {//Left sends flow to right
                let mut vr = 16.0f64.min(loopLeft.loop_pressure.get::<psi>() * 0.0058) / 60.0;

                //Limiting available flow with maximum flow capacity of all pumps of the loop.
                //This is a workaround to limit PTU greed for flow
                vr=vr.min(loopLeft.current_max_flow.get::<gallon_per_second>()*Ptu::AGRESSIVENESS_FACTOR);

                //Low pass on flow
                vr = Ptu::FLOW_DYNAMIC_LOW_PASS_LEFT_SIDE * vr
                + (1.0-Ptu::FLOW_DYNAMIC_LOW_PASS_LEFT_SIDE) * self.last_flow.get::<gallon_per_second>();

                self.flow_to_left= VolumeRate::new::<gallon_per_second>(-vr);
                self.flow_to_right= VolumeRate::new::<gallon_per_second>(vr * 0.81);
                self.last_flow=VolumeRate::new::<gallon_per_second>(vr);

                self.isActiveLeft=true;
            } else if self.isActiveRight || (!self.isActiveLeft && deltaP.get::<psi>()  < -500.0) {//Right sends flow to left
                let mut vr = 34.0f64.min(loopRight.loop_pressure.get::<psi>() * 0.0125) / 60.0;

                //Limiting available flow with maximum flow capacity of all pumps of the loop.
                //This is a workaround to limit PTU greed for flow
                vr=vr.min(loopRight.current_max_flow.get::<gallon_per_second>()*Ptu::AGRESSIVENESS_FACTOR);

                //Low pass on flow
                vr = Ptu::FLOW_DYNAMIC_LOW_PASS_RIGHT_SIDE * vr
                + (1.0-Ptu::FLOW_DYNAMIC_LOW_PASS_RIGHT_SIDE) * self.last_flow.get::<gallon_per_second>();

                self.flow_to_left = VolumeRate::new::<gallon_per_second>(vr * 0.70);
                self.flow_to_right= VolumeRate::new::<gallon_per_second>(-vr);
                self.last_flow=VolumeRate::new::<gallon_per_second>(vr);

                self.isActiveRight=true;
            }

            //TODO REVIEW DEACTICATION LOGIC
            if  self.isActiveRight && loopLeft.loop_pressure.get::<psi>()  > 3001.0
             || self.isActiveLeft && loopRight.loop_pressure.get::<psi>() > 3001.0
             || self.isActiveRight && loopRight.loop_pressure.get::<psi>()  < 500.0
             || self.isActiveLeft && loopLeft.loop_pressure.get::<psi>()  < 500.0
             {
                self.flow_to_left=VolumeRate::new::<gallon_per_second>(0.0);
                self.flow_to_right=VolumeRate::new::<gallon_per_second>(0.0);
                self.isActiveRight=false;
                self.isActiveLeft=false;
                self.last_flow = VolumeRate::new::<gallon_per_second>(0.0);
            }
        }
    }

    pub fn enabling (&mut self , enable_flag:bool){
        self.isEnabled = enable_flag;
    }
}

pub struct HydLoop {
    fluid: HydFluid,
    accumulator_gas_pressure: Pressure,
    accumulator_gas_volume: Volume,
    accumulator_fluid_volume: Volume,
    accumulator_press_breakpoints:[f64; 9] ,
    accumulator_flow_carac:[f64; 9] ,
    color: LoopColor,
    connected_to_ptu_left_side: bool,
    connected_to_ptu_right_side: bool,
    loop_pressure: Pressure,
    loop_volume: Volume,
    max_loop_volume: Volume,
    high_pressure_volume : Volume,
    ptu_active: bool,
    reservoir_volume: Volume,
    current_delta_vol: Volume,
    current_flow: VolumeRate,
    current_max_flow : VolumeRate, //Current total max flow available from pressure sources
}

impl HydLoop {
    const ACCUMULATOR_GAS_PRE_CHARGE: f64 =1885.0; // Nitrogen PSI
    const ACCUMULATOR_MAX_VOLUME: f64  =0.264; // in gallons
    //const HYDRAULIC_FLUID_DENSITY: f64 = 1000.55; // Exxon Hyjet IV, kg/m^3

    //Low pass filter on pressure. This has to be pretty high not to modify behavior of the loop, but still dampening numerical instability
    const PRESSURE_LOW_PASS_FILTER : f64 = 0.75;

    const DELTA_VOL_LOW_PASS_FILTER : f64 = 0.1;

    const ACCUMULATOR_PRESS_BREAKPTS: [f64; 9] = [
        0.0 ,5.0 , 10.0 ,50.0 ,100.0 ,200.0 ,500.0 ,1000.0 , 10000.0
    ];
    const ACCUMULATOR_FLOW_CARAC: [f64; 9] = [
        0.0,0.005, 0.008, 0.01, 0.02, 0.08,  0.15,   0.35 ,   0.5
    ];

    pub fn new(
        color: LoopColor,
        connected_to_ptu_left_side: bool, //Is connected to PTU "left" side: non variable displacement side
        connected_to_ptu_right_side: bool, //Is connected to PTU "right" side: variable displacement side
        loop_volume: Volume,
        max_loop_volume: Volume,
        high_pressure_volume: Volume,
        reservoir_volume: Volume,
        fluid:HydFluid,
    ) -> HydLoop {
        HydLoop {
            accumulator_gas_pressure: Pressure::new::<psi>(HydLoop::ACCUMULATOR_GAS_PRE_CHARGE),
            accumulator_gas_volume: Volume::new::<gallon>(HydLoop::ACCUMULATOR_MAX_VOLUME),
            accumulator_fluid_volume: Volume::new::<gallon>(0.),
            color,
            connected_to_ptu_left_side,
            connected_to_ptu_right_side,
            loop_pressure: Pressure::new::<psi>(14.7),
            loop_volume,
            max_loop_volume,
            high_pressure_volume,
            ptu_active: false,
            reservoir_volume,
            fluid,
            current_delta_vol: Volume::new::<gallon>(0.),
            current_flow: VolumeRate::new::<gallon_per_second>(0.),
            accumulator_press_breakpoints:HydLoop::ACCUMULATOR_PRESS_BREAKPTS,
            accumulator_flow_carac:HydLoop::ACCUMULATOR_FLOW_CARAC,
            current_max_flow: VolumeRate::new::<gallon_per_second>(0.),
        }
    }

    pub fn get_pressure(&self) -> Pressure {
        self.loop_pressure
    }

    pub fn get_reservoir_volume(&self) -> Volume {
        self.reservoir_volume
    }

    pub fn get_usable_reservoir_fluid(&self, amount: Volume) -> Volume {
        let mut drawn = amount;
        if amount > self.reservoir_volume {
            drawn = self.reservoir_volume;
        }
        drawn
    }

    //Returns the max flow that can be output from reservoir in dt time
    pub fn get_usable_reservoir_flow(&self, amount: VolumeRate, delta_time: Time) -> VolumeRate {
        let mut drawn = amount;

        let max_flow= self.reservoir_volume / delta_time;
        if amount > max_flow {
            drawn = max_flow;
        }
        drawn
    }

    //Method to update pressure of a loop. The more delta volume is added, the more pressure rises
    //Directly from bulk modulus equation
    pub fn delta_pressure_from_delta_volume(&self, delta_vol: Volume) -> Pressure {
            return delta_vol / self.high_pressure_volume * self.fluid.get_bulk_mod();
    }

    //Gives the exact volume of fluid needed to get to any target_press pressure
    pub fn vol_to_target(&self,target_press : Pressure) -> Volume {
        (target_press-self.loop_pressure) * (self.high_pressure_volume) / self.fluid.get_bulk_mod()
    }


    pub fn update(
        &mut self,
        delta_time : &Duration,
        context: &UpdateContext,
        electric_pumps: Vec<&ElectricPump>,
        engine_driven_pumps: Vec<&EngineDrivenPump>,
        ram_air_pumps: Vec<&RatPump>,
        ptus: Vec<&Ptu>,
    ) {
        let mut pressure = self.loop_pressure;
        let mut delta_vol_max = Volume::new::<gallon>(0.);
        let mut delta_vol_min = Volume::new::<gallon>(0.);
        let mut reservoir_return =Volume::new::<gallon>(0.);
        let mut delta_vol = Volume::new::<gallon>(0.);

        for p in engine_driven_pumps {
            delta_vol_max += p.get_delta_vol_max();
            delta_vol_min += p.get_delta_vol_min();
        }
        for p in electric_pumps {
            delta_vol_max += p.get_delta_vol_max();
            delta_vol_min += p.get_delta_vol_min();
        }
        for p in ram_air_pumps {
            delta_vol_max += p.get_delta_vol_max();
            delta_vol_min += p.get_delta_vol_min();
        }

        //Storing max pump capacity available. for now used in PTU model to limit it's input flow
        self.current_max_flow = delta_vol_max / Time::new::<second>(delta_time.as_secs_f64());

        //Static leaks
        //TODO: separate static leaks per zone of high pressure or actuator
        //TODO: Use external pressure and/or reservoir pressure instead of 14.7 psi default
        let static_leaks_vol = Volume::new::<gallon>(0.04 * delta_time.as_secs_f64() * (self.loop_pressure.get::<psi>() - 14.7) / 3000.0);

        // Draw delta_vol from reservoir
        delta_vol -= static_leaks_vol;
        reservoir_return += static_leaks_vol;

        //PTU flows handling
        let mut ptu_act = false;
        for ptu in ptus {
            let mut actualFlow = VolumeRate::new::<gallon_per_second>(0.0);
            if self.connected_to_ptu_left_side {
                if ptu.isActiveLeft || ptu.isActiveLeft {
                    ptu_act = true;
                }
                if ptu.flow_to_left > VolumeRate::new::<gallon_per_second>(0.0) {
                    //were are left side of PTU and positive flow so we receive flow using own reservoir
                    actualFlow=self.get_usable_reservoir_flow(ptu.flow_to_left,Time::new::<second>(delta_time.as_secs_f64()));
                    self.reservoir_volume-=actualFlow* Time::new::<second>(delta_time.as_secs_f64());
                } else  {
                    //we are using own flow to power right side so we send that back
                    //to our own reservoir
                    actualFlow=ptu.flow_to_left;
                    reservoir_return-=actualFlow* Time::new::<second>(delta_time.as_secs_f64());
                }
                delta_vol+=actualFlow * Time::new::<second>(delta_time.as_secs_f64());
            } else if self.connected_to_ptu_right_side {
                 if ptu.isActiveLeft || ptu.isActiveLeft {
                    ptu_act = true;
                }
                if ptu.flow_to_right > VolumeRate::new::<gallon_per_second>(0.0) {
                    //were are right side of PTU and positive flow so we receive flow using own reservoir
                    actualFlow=self.get_usable_reservoir_flow(ptu.flow_to_right,Time::new::<second>(delta_time.as_secs_f64()));
                    self.reservoir_volume-=actualFlow* Time::new::<second>(delta_time.as_secs_f64());
                } else {
                    //we are using own flow to power left side so we send that back
                    //to our own reservoir
                    actualFlow=ptu.flow_to_right;
                    reservoir_return-=actualFlow* Time::new::<second>(delta_time.as_secs_f64());
                }
                delta_vol+=actualFlow* Time::new::<second>(delta_time.as_secs_f64());
            }
        }
        self.ptu_active = ptu_act;
        //END PTU

        //Priming the loop if not filled in
        //TODO bug, ptu can't prime the loop is it is not providing flow through delta_vol_max
        if self.loop_volume < self.max_loop_volume { //} %TODO what to do if we are back under max volume and unprime the loop?
            let difference =  self.max_loop_volume  - self.loop_volume;
            // println!("---Priming diff {}", difference.get::<gallon>());
            let availableFluidVol=self.reservoir_volume.min(delta_vol_max);
            let delta_loop_vol = availableFluidVol.min(difference);
            delta_vol_max -= delta_loop_vol;//%TODO check if we cross the deltaVolMin?
            self.loop_volume+= delta_loop_vol;
            self.reservoir_volume -= delta_loop_vol;
            // println!("---Priming vol {} / {}", self.loop_volume.get::<gallon>(),self.max_loop_volume.get::<gallon>());
        } else {
            // println!("---Primed {}", self.loop_volume.get::<gallon>());
        }
        //end priming


        //ACCUMULATOR
        let accumulatorDeltaPress = self.accumulator_gas_pressure - self.loop_pressure;
        let flowVariation = VolumeRate::new::<gallon_per_second>(interpolation(&self.accumulator_press_breakpoints,&self.accumulator_flow_carac,accumulatorDeltaPress.get::<psi>().abs()));

        //TODO HANDLE OR CHECK IF RESERVOIR AVAILABILITY is OK
        //TODO check if accumulator can be used as a min/max flow producer to
        //avoid it being a consumer that might unsettle pressure
        if  accumulatorDeltaPress.get::<psi>() > 0.0  {
            let volumeFromAcc = self.accumulator_fluid_volume.min(flowVariation * Time::new::<second>(delta_time.as_secs_f64()));
            self.accumulator_fluid_volume -= volumeFromAcc;
            self.accumulator_gas_volume += volumeFromAcc;
            delta_vol += volumeFromAcc;
        } else {
            let volumeToAcc = delta_vol.max(Volume::new::<gallon>(0.0)).max(flowVariation * Time::new::<second>(delta_time.as_secs_f64()));
            self.accumulator_fluid_volume += volumeToAcc;
            self.accumulator_gas_volume -= volumeToAcc;
            delta_vol -= volumeToAcc;
        }

        self.accumulator_gas_pressure = (Pressure::new::<psi>(HydLoop::ACCUMULATOR_GAS_PRE_CHARGE) * Volume::new::<gallon>(HydLoop::ACCUMULATOR_MAX_VOLUME)) / (Volume::new::<gallon>(HydLoop::ACCUMULATOR_MAX_VOLUME) - self.accumulator_fluid_volume);
        //END ACCUMULATOR



        //Actuators
        let used_fluidQty= Volume::new::<gallon>(0.); // %%total fluid used
        //foreach actuator
            //used_fluidQty =used_fluidQty+aileron.volumeToActuatorAccumulated*264.172; %264.172 is m^3 to gallons
            //reservoirReturn=reservoirReturn+aileron.volumeToResAccumulated*264.172;
            //actuator.resetVolumes()
            //actuator.set_available_pressure(self.loop_pressure)
         //end foreach
        //end actuator

        delta_vol -= used_fluidQty;


        //How much we need to reach target of 3000?
        let mut volume_needed_to_reach_pressure_target = self.vol_to_target(Pressure::new::<psi>(3000.0));
        //Actually we need this PLUS what is used by consumers.
        volume_needed_to_reach_pressure_target -= delta_vol;

        //Now computing what we will actually use from flow providers limited by
        //their min and max flows and reservoir availability
        let actual_volume_added_to_pressurise = self.reservoir_volume.min(delta_vol_min.max(delta_vol_max.min(volume_needed_to_reach_pressure_target)));
        delta_vol+=actual_volume_added_to_pressurise;

        //Loop Pressure update From Bulk modulus
        let press_delta = self.delta_pressure_from_delta_volume(delta_vol);
        let new_raw_press=self.loop_pressure + press_delta; //New raw pressure before we filter it

        self.loop_pressure= HydLoop::PRESSURE_LOW_PASS_FILTER * new_raw_press + (1.-HydLoop::PRESSURE_LOW_PASS_FILTER) * self.loop_pressure;
        self.loop_pressure = self.loop_pressure.max(Pressure::new::<psi>(14.7)); //Forcing a min pressure


        //Update reservoir
        self.reservoir_volume -= actual_volume_added_to_pressurise; //%limit to 0 min? for case of negative added?
        self.reservoir_volume += reservoir_return;

        //Update Volumes

        //Low pass filter on final delta vol to help with stability and final flow noise
        delta_vol = HydLoop::DELTA_VOL_LOW_PASS_FILTER * delta_vol + (1.-HydLoop::DELTA_VOL_LOW_PASS_FILTER ) * self.current_delta_vol;
        self.loop_volume += delta_vol;

        self.current_delta_vol=delta_vol;
        self.current_flow=delta_vol / Time::new::<second>(delta_time.as_secs_f64());
    }
}

////////////////////////////////////////////////////////////////////////////////
// PUMP DEFINITION
////////////////////////////////////////////////////////////////////////////////

pub struct Pump {
    delta_vol_max: Volume,
    delta_vol_min: Volume,
    pressBreakpoints:[f64; 9] ,
    displacementCarac:[f64; 9] ,
    displacement_dynamic: f64, //Displacement low pass filter. [0:1], 0 frozen -> 1 instantaneous dynamic
}
impl Pump {
    fn new(pressBreakpoints:[f64; 9],displacementCarac:[f64; 9],displacement_dynamic:f64) -> Pump {
        Pump {
            delta_vol_max: Volume::new::<gallon>(0.),
            delta_vol_min: Volume::new::<gallon>(0.),
            pressBreakpoints:pressBreakpoints,
            displacementCarac:displacementCarac,
            displacement_dynamic:displacement_dynamic,
        }
    }

    fn update(&mut self, delta_time: &Duration,context: &UpdateContext, line: &HydLoop, rpm: f64) {
        let displacement = self.calculate_displacement(line.get_pressure());

        let flow = Pump::calculate_flow(rpm, displacement);

        self.delta_vol_max= (1.0 - self.displacement_dynamic)*self.delta_vol_max + self.displacement_dynamic * flow * Time::new::<second>(delta_time.as_secs_f64());
        self.delta_vol_min=Volume::new::<gallon>(0.0);
    }

    fn calculate_displacement(&self , pressure: Pressure) -> Volume {
        Volume::new::<cubic_inch>(interpolation(&self.pressBreakpoints,&self.displacementCarac,pressure.get::<psi>()))
    }

    fn calculate_flow(rpm: f64, displacement: Volume) -> VolumeRate {
        VolumeRate::new::<gallon_per_second>(rpm * displacement.get::<cubic_inch>() / 231.0 / 60.0)
    }
}
impl PressureSource for Pump {
    fn get_delta_vol_max(&self) -> Volume {
        self.delta_vol_max
    }

    fn get_delta_vol_min(&self) -> Volume {
        self.delta_vol_min
    }
}

pub struct ElectricPump {
    active: bool,
    rpm: f64,
    pump: Pump,
}
impl ElectricPump {
    const SPOOLUP_TIME: f64 = 4.0;
    const SPOOLDOWN_TIME: f64 = 4.0;
    const NOMINAL_SPEED: f64 = 7600.0;
    const DISPLACEMENT_BREAKPTS: [f64; 9] = [
        0.0, 500.0, 1000.0, 1500.0, 2800.0, 2900.0, 3000.0, 3050.0, 3500.0,
    ];
    const DISPLACEMENT_MAP: [f64; 9] = [
        0.263,0.263,0.263,  0.263 , 0.263,  0.263 , 0.163,  0.0 ,   0.0
    ];
    const DISPLACEMENT_DYNAMICS: f64 = 1.0; //1 == No filtering

    pub fn new() -> ElectricPump {
        ElectricPump {
            active: false,
            rpm: 0.,
            pump: Pump::new(ElectricPump::DISPLACEMENT_BREAKPTS,ElectricPump::DISPLACEMENT_MAP,ElectricPump::DISPLACEMENT_DYNAMICS),
        }
    }

    pub fn start(&mut self) {
        self.active = true;
    }

    pub fn stop(&mut self) {
        self.active = false;
    }

    pub fn update(&mut self,delta_time: &Duration, context: &UpdateContext, line: &HydLoop) {
        //TODO Simulate speed of pump depending on pump load (flow?/ current?)
        //Pump startup/shutdown process
        if self.active && self.rpm < ElectricPump::NOMINAL_SPEED {
            self.rpm += (ElectricPump::NOMINAL_SPEED / ElectricPump::SPOOLUP_TIME) * delta_time.as_secs_f64();
        } else if !self.active && self.rpm > 0.0 {
            self.rpm -= (ElectricPump::NOMINAL_SPEED / ElectricPump::SPOOLDOWN_TIME) * delta_time.as_secs_f64();
        }

        //Limiting min and max speed
        self.rpm = self.rpm.min(ElectricPump::NOMINAL_SPEED ).max(0.0);

        self.pump.update(delta_time, context, line, self.rpm);
    }
}
impl PressureSource for ElectricPump {
    fn get_delta_vol_max(&self) -> Volume {
        self.pump.get_delta_vol_max()
    }
    fn get_delta_vol_min(&self) -> Volume {
        self.pump.get_delta_vol_min()
    }
}

pub struct EngineDrivenPump {
    active: bool,
    pump: Pump,
}
impl EngineDrivenPump {
    const LEAP_1A26_MAX_N2_RPM: f64 = 16645.0;
    const DISPLACEMENT_BREAKPTS: [f64; 9] = [
        0.0, 500.0, 1000.0, 1500.0, 2800.0, 2900.0, 3000.0, 3050.0, 3500.0,
    ];
    const DISPLACEMENT_MAP: [f64; 9] = [
        2.4 ,2.4,   2.4,    2.4 ,   2.4,    2.4 ,   2.0,    0.0 ,   0.0 ];
    const MAX_RPM: f64 = 4000.;

    const DISPLACEMENT_DYNAMICS: f64 = 0.05; //0.1 == 90% filtering on max displacement transient

    pub fn new() -> EngineDrivenPump {
        EngineDrivenPump {
            active: false,
            pump: Pump::new(EngineDrivenPump::DISPLACEMENT_BREAKPTS,
                EngineDrivenPump::DISPLACEMENT_MAP,
                EngineDrivenPump::DISPLACEMENT_DYNAMICS,
            ),
        }
    }

    pub fn update(&mut self, delta_time : &Duration,context: &UpdateContext, line: &HydLoop, engine: &Engine) {
        let mut rpm = EngineDrivenPump::MAX_RPM.min(engine.n2.get::<percent>().powi(2)*0.08*EngineDrivenPump::MAX_RPM / 100.0);

        //TODO Activate pumps realistically, maybe with a displacement rate limited when activated/deactivated?
        if !self.active{ //Hack for pump activation
            rpm = 0.0;
        }
        self.pump.update(delta_time,context, line, rpm);
    }

    pub fn start(&mut self ) {
        self.active=true;
    }

    pub fn stop(&mut self ) {
        self.active=false;
    }
}
impl PressureSource for EngineDrivenPump {
    fn get_delta_vol_min(&self) -> Volume {
        self.pump.get_delta_vol_min()
    }
    fn get_delta_vol_max(&self) -> Volume {
        self.pump.get_delta_vol_max()
    }
}

pub struct RatPump {
    active: bool,
    pump: Pump,
}
impl RatPump {
    const DISPLACEMENT_BREAKPTS: [f64; 9] = [
        0.0, 500.0, 1000.0, 1500.0, 2800.0, 2900.0, 3000.0, 3050.0, 3500.0,
    ];
    const DISPLACEMENT_MAP: [f64; 9] = [
        1.15 , 1.15,  1.15,  1.15 , 1.15,  1.15 , 0.9, 0.0 ,0.0
    ];

    const NORMAL_RPM: f64 = 6000.;

    const DISPLACEMENT_DYNAMICS: f64 = 1.0; //1 == no filtering

    pub fn new() -> RatPump {
        RatPump {
            active: false,
            pump: Pump::new(RatPump::DISPLACEMENT_BREAKPTS,RatPump::DISPLACEMENT_MAP, RatPump::DISPLACEMENT_DYNAMICS),
        }
    }

    pub fn update(&mut self, delta_time: &Duration,context: &UpdateContext, line: &HydLoop) {
        self.pump.update(delta_time, context, line, RatPump::NORMAL_RPM);
    }
}
impl PressureSource for RatPump {
    fn get_delta_vol_max(&self) -> Volume {
        self.pump.get_delta_vol_max()
    }

    fn get_delta_vol_min(&self) -> Volume {
        self.pump.get_delta_vol_min()
    }
}

////////////////////////////////////////////////////////////////////////////////
// ACTUATOR DEFINITION
////////////////////////////////////////////////////////////////////////////////

pub struct Actuator {
    a_type: ActuatorType,
    active: bool,
    affected_by_gravity: bool,
    area: Area,
    line: HydLoop,
    neutral_is_zero: bool,
    stall_load: Force,
    volume_used_at_max_deflection: Volume,
}

// TODO
impl Actuator {
    pub fn new(a_type: ActuatorType, line: HydLoop) -> Actuator {
        Actuator {
            a_type,
            active: false,
            affected_by_gravity: false,
            area: Area::new::<square_meter>(5.0),
            line,
            neutral_is_zero: true,
            stall_load: Force::new::<newton>(47000.),
            volume_used_at_max_deflection: Volume::new::<gallon>(0.),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// TESTS
////////////////////////////////////////////////////////////////////////////////


use plotlib::page::Page;
use plotlib::repr::Plot;
use plotlib::view::ContinuousView;
use plotlib::style::{PointMarker, PointStyle, LineStyle};

extern crate rustplotlib;
use rustplotlib::Figure;


fn make_figure<'a>(h: &'a History) -> Figure<'a> {
    use rustplotlib::{Axes2D, Line2D};

    let mut allAxis: Vec<Option<Axes2D>> = Vec::new();

    let mut idx=0;
    for curData in &h.dataVector {
        let mut currAxis = Axes2D::new()
            .add(Line2D::new(h.nameVector[idx].as_str())
            .data(&h.timeVector, &curData)
            .color("blue")
            //.marker("x")
            //.linestyle("--")
            .linewidth(1.0))
            .xlabel("Time [sec]")
            .ylabel(h.nameVector[idx].as_str())
            .legend("best")
            .xlim(0.0, *h.timeVector.last().unwrap());
            //.ylim(-2.0, 2.0);

            currAxis=currAxis.grid(true);
        idx=idx+1;
        allAxis.push(Some(currAxis));
    }

    Figure::new()
      .subplots(allAxis.len() as u32, 1, allAxis)
  }

//History class to record a simulation
pub struct History {
    timeVector: Vec<f64>, //Simulation time starting from 0
    nameVector: Vec<String>, //Name of each var saved
    dataVector: Vec<Vec<f64>>, //Vector data for each var saved
    dataSize: usize,
}

impl History {
    pub fn new(names: Vec<String> ) -> History {
        History {
            timeVector: Vec::new(),
            nameVector: names.clone(),
            dataVector: Vec::new(),
            dataSize: names.len(),
        }
    }

    //Sets initialisation values of each data before first step
    pub fn init(&mut self,startTime:f64, values: Vec<f64>) {
        self.timeVector.push(startTime);
        for idx in 0..(values.len()) {
            self.dataVector.push(vec![values[idx]]);
        }
    }

    //Updates all values and time vector
    pub fn update(&mut self,deltaTime :f64, values: Vec<f64>) {
        self.timeVector.push(self.timeVector.last().unwrap() + deltaTime);
        self.pushData(values);
    }

    pub fn pushData(&mut self,values: Vec<f64>){
        for idx in 0..values.len() {
            self.dataVector[idx].push(values[idx]);
        }
    }

    //Builds a graph using rust crate plotlib
    pub fn show(self){

        let mut v = ContinuousView::new()
        .x_range(0.0, *self.timeVector.last().unwrap())
        .y_range(0.0, 3500.0)
        .x_label("Time (s)")
        .y_label("Value");

        for curData in self.dataVector {
            //Here build the 2 by Xsamples vector
            let mut newVector: Vec<(f64,f64)> = Vec::new();
            for sampleIdx in 0..self.timeVector.len(){
                newVector.push( (self.timeVector[sampleIdx] , curData[sampleIdx]) );
            }

            // We create our scatter plot from the data
            let s1: Plot = Plot::new(newVector).line_style(
                LineStyle::new()
                    .colour("#DD3355"),
            );

            v=v.add(s1);
        }


        // A page with a single view is then saved to an SVG file
        Page::single(&v).save("scatter.svg").unwrap();

    }

    //builds a graph using matplotlib python backend. PYTHON REQUIRED AS WELL AS MATPLOTLIB PACKAGE
    pub fn showMatplotlib(&self,figure_title : &str){
        let fig = make_figure(&self);

        use rustplotlib::Backend;
        use rustplotlib::backend::Matplotlib;
        let mut mpl = Matplotlib::new().unwrap();
        mpl.set_style("ggplot").unwrap();

        fig.apply(&mut mpl).unwrap();

        //mpl.savefig("simple.png").unwrap();
        mpl.savefig(figure_title);
        //mpl.dump_pickle("simple.fig.pickle").unwrap();
        mpl.wait().unwrap();
    }
}

#[cfg(test)]
mod tests {
    //use uom::si::volume_rate::VolumeRate;

    use super::*;
    #[test]
    //Runs engine driven pump, checks pressure OK, shut it down, check drop of pressure after 20s
    fn green_loop_edp_simulation() {
        let green_loop_var_names = vec!["Loop Pressure".to_string(), "Loop Volume".to_string(), "Loop Reservoir".to_string(), "Loop Flow".to_string()];
        let mut greenLoopHistory = History::new(green_loop_var_names);

        let edp1_var_names = vec!["Delta Vol Max".to_string(), "n2 ratio".to_string()];
        let mut edp1_History = History::new(edp1_var_names);

        let mut edp1 = engine_driven_pump();
        let mut green_loop = hydraulic_loop(LoopColor::Green);
        edp1.active = true;

        let init_n2 = Ratio::new::<percent>(55.0);
        let mut engine1 = engine(init_n2);
        let ct = context(Duration::from_millis(100));

        let green_acc_var_names = vec!["Loop Pressure".to_string(), "Acc gas press".to_string(), "Acc fluid vol".to_string(),"Acc gas vol".to_string()];
        let mut accuGreenHistory = History::new(green_acc_var_names);

        greenLoopHistory.init(0.0,vec![green_loop.loop_pressure.get::<psi>(), green_loop.loop_volume.get::<gallon>(),green_loop.reservoir_volume.get::<gallon>(),green_loop.current_flow.get::<gallon_per_second>()]);
        edp1_History.init(0.0,vec![edp1.get_delta_vol_max().get::<liter>(), engine1.n2.get::<percent>() as f64]);
        accuGreenHistory.init(0.0,vec![green_loop.loop_pressure.get::<psi>(), green_loop.accumulator_gas_pressure.get::<psi>() ,green_loop.accumulator_fluid_volume.get::<gallon>(),green_loop.accumulator_gas_volume.get::<gallon>()]);
        for x in 0..600 {
            if x == 50 { //After 5s
                assert!(green_loop.loop_pressure >= Pressure::new::<psi>(2950.0));
            }
            if x == 200 {
                assert!(green_loop.loop_pressure >= Pressure::new::<psi>(2950.0));
                edp1.stop();
            }
            if x >= 500 { //Shutdown + 30s
                assert!(green_loop.loop_pressure <= Pressure::new::<psi>(250.0));
            }

            edp1.update(&ct.delta,&ct, &green_loop, &engine1);
            green_loop.update(&ct.delta,&ct, Vec::new(), vec![&edp1], Vec::new(), Vec::new());
            if x % 20 == 0 {
                println!("Iteration {}", x);
                println!("-------------------------------------------");
                println!("---PSI: {}", green_loop.loop_pressure.get::<psi>());
                println!(
                    "--------Reservoir Volume (g): {}",
                    green_loop.reservoir_volume.get::<gallon>()
                );
                println!(
                    "--------Loop Volume (g): {}",
                    green_loop.loop_volume.get::<gallon>()
                );
                println!(
                    "--------Acc Fluid Volume (L): {}",
                    green_loop.accumulator_fluid_volume.get::<liter>()
                );
                println!(
                    "--------Acc Gas Volume (L): {}",
                    green_loop.accumulator_gas_volume.get::<liter>()
                );
                println!(
                    "--------Acc Gas Pressure (psi): {}",
                    green_loop.accumulator_gas_pressure.get::<psi>()
                );
            }

            greenLoopHistory.update(ct.delta.as_secs_f64(), vec![green_loop.loop_pressure.get::<psi>(), green_loop.loop_volume.get::<gallon>(),green_loop.reservoir_volume.get::<gallon>(),green_loop.current_flow.get::<gallon_per_second>()]);
            edp1_History.update(ct.delta.as_secs_f64(),vec![edp1.get_delta_vol_max().get::<liter>(), engine1.n2.get::<percent>() as f64]);
            accuGreenHistory.update(ct.delta.as_secs_f64(),vec![green_loop.loop_pressure.get::<psi>(), green_loop.accumulator_gas_pressure.get::<psi>() ,green_loop.accumulator_fluid_volume.get::<gallon>(),green_loop.accumulator_gas_volume.get::<gallon>()]);

        }
        assert!(true);

        greenLoopHistory.showMatplotlib("green_loop_edp_simulation_press");
        edp1_History.showMatplotlib("green_loop_edp_simulation_EDP1 data") ;
        accuGreenHistory.showMatplotlib("green_loop_edp_simulation_Green Accum data") ;
    }

    #[test]
    //Tests fixed step mechanism as implemented in A320Hydraulics
    fn fixed_step_loop_test() {
        use rand::Rng;

        let mut edp1 = engine_driven_pump();
        let mut green_loop = hydraulic_loop(LoopColor::Green);
        edp1.active = true;

        let init_n2 = Ratio::new::<percent>(0.5);
        let mut engine1 = engine(init_n2);

        let mut rng = rand::thread_rng();
        let mut real_time=Duration::from_millis(0);
        let mut ct = context(Duration::from_millis(rng.gen_range(2..110)));

        let min_hyd_loop_timestep = Duration::from_millis(100); //Hyd Sim rate = 10 Hz
        let mut total_sim_time_elapsed=Duration::from_millis(0);
        let mut lag_time_accumulator =Duration::from_millis(0);

        while real_time < Duration::from_secs_f64(5.0)  {
            ct.delta = Duration::from_millis(rng.gen_range(2..110));
            real_time+=ct.delta;
            //println!("CALLED DELTA {:.3}", ct.delta.as_secs_f64());
            //println!("Real time: {:.3}", real_time.as_secs_f64());

            total_sim_time_elapsed+=ct.delta;
            let time_to_catch=ct.delta + lag_time_accumulator;

            let numberOfSteps_f64 = time_to_catch.as_secs_f64()/min_hyd_loop_timestep.as_secs_f64();

            assert!(lag_time_accumulator.as_secs_f64() < 0.2);
            assert!(numberOfSteps_f64 < 5.0);
            if numberOfSteps_f64 < 1.0 {
                //Can't do a full time step
                //we can either do an update with smaller step or wait next iteration
                //Other option is to update only actuator position based on known hydraulic
                //state to avoid lag of control surfaces if sim runs really fast
                lag_time_accumulator=Duration::from_secs_f64(numberOfSteps_f64 * min_hyd_loop_timestep.as_secs_f64()); //Time lag is float part of num of steps * fixed time step to get a result in time
            } else {
                //TRUE UPDATE LOOP HERE
                let num_of_update_loops = numberOfSteps_f64.floor() as u32; //Int part is the actual number of loops to do
                //Rest of floating part goes into accumulator
                lag_time_accumulator= Duration::from_secs_f64((numberOfSteps_f64 - (num_of_update_loops as f64))* min_hyd_loop_timestep.as_secs_f64()); //Keep track of time left after all fixed loop are done


                //UPDATING HYDRAULICS AT FIXED STEP
                for curLoop in  0..num_of_update_loops {
                    //UPDATE HYDRAULICS FIXED TIME STEP
                    edp1.update(&ct.delta,&ct, &green_loop, &engine1);
                    green_loop.update(&ct.delta,&ct, Vec::new(), vec![&edp1], Vec::new(), Vec::new());
                    //println!("---PSI: {}", green_loop.loop_pressure.get::<psi>());
                    //println!("---Sim time: {:.3}", total_sim_time_elapsed.as_secs_f64());
                    //println!("---Lag time: {:.3}", lag_time_accumulator.as_secs_f64());
                    //println!("---num_of_update_loops: {:.1}",num_of_update_loops);
                }
            }

        }

        assert!(lag_time_accumulator.as_secs_f64() < 1.0);
        assert!((real_time - total_sim_time_elapsed).as_secs_f64().abs()  < 0.2);

     }


    #[test]
    //Runs electric pump, checks pressure OK, shut it down, check drop of pressure after 20s
    fn yellow_loop_epump_simulation() {
        let mut epump = electric_pump();
        let mut yellow_loop = hydraulic_loop(LoopColor::Yellow);
        epump.active = true;

        let ct = context(Duration::from_millis(100));
        for x in 0..800 {
            if x == 400 {
                assert!(yellow_loop.loop_pressure >= Pressure::new::<psi>(2800.0));
                epump.active = false;
            }

            if x >= 600 { //X+200 after shutoff = X + 20seconds @ 100ms, so pressure shall be low
                assert!(yellow_loop.loop_pressure <= Pressure::new::<psi>(200.0));
            }
            epump.update(&ct.delta,&ct, &yellow_loop);
            yellow_loop.update(&ct.delta,&ct, vec![&epump], Vec::new(), Vec::new(), Vec::new());
            if x % 20 == 0 {
                println!("Iteration {}", x);
                println!("-------------------------------------------");
                println!("---PSI: {}", yellow_loop.loop_pressure.get::<psi>());
                println!("---RPM: {}", epump.rpm);
                println!(
                    "--------Reservoir Volume (g): {}",
                    yellow_loop.reservoir_volume.get::<gallon>()
                );
                println!(
                    "--------Loop Volume (g): {}",
                    yellow_loop.loop_volume.get::<gallon>()
                );
                println!(
                    "--------Acc Volume (g): {}",
                    yellow_loop.accumulator_gas_volume.get::<gallon>()
                );
            }
        }

        assert!(true)
    }

    #[test]
    //Runs electric pump, checks pressure OK, shut it down, check drop of pressure after 20s
    fn blue_loop_epump_simulation() {
        let mut epump = electric_pump();
        let mut blue_loop = hydraulic_loop(LoopColor::Blue);
        epump.active = true;

        let ct = context(Duration::from_millis(100));
        for x in 0..800 {
            if x == 400 {
                assert!(blue_loop.loop_pressure >= Pressure::new::<psi>(2800.0));
                epump.active = false;
            }

            if x >= 600 { //X+200 after shutoff = X + 20seconds @ 100ms, so pressure shall be low
                assert!(blue_loop.loop_pressure <= Pressure::new::<psi>(100.0));
            }
            epump.update(&ct.delta,&ct, &blue_loop);
            blue_loop.update(&ct.delta,&ct, vec![&epump], Vec::new(), Vec::new(), Vec::new());
            if x % 20 == 0 {
                println!("Iteration {}", x);
                println!("-------------------------------------------");
                println!("---PSI: {}", blue_loop.loop_pressure.get::<psi>());
                println!("---RPM: {}", epump.rpm);
                println!(
                    "--------Reservoir Volume (g): {}",
                    blue_loop.reservoir_volume.get::<gallon>()
                );
                println!(
                    "--------Loop Volume (g): {}",
                    blue_loop.loop_volume.get::<gallon>()
                );
                println!(
                    "--------Acc Volume (g): {}",
                    blue_loop.accumulator_gas_volume.get::<gallon>()
                );
            }
        }

        assert!(true)
    }

    #[test]
    //Runs green edp and yellow epump, checks pressure OK,
    //shut green edp off, check drop of pressure and ptu effect
    //shut yellow epump, check drop of pressure in both loops
    fn yellow_green_ptu_loop_simulation() {
        let loop_var_names = vec!["GREEN Loop Pressure".to_string(), "YELLOW Loop Pressure".to_string(),"GREEN Loop reservoir".to_string(), "YELLOW Loop reservoir".to_string(), "GREEN Loop delta vol".to_string(),"YELLOW Loop delta vol".to_string()];
        let mut LoopHistory = History::new(loop_var_names);

        let ptu_var_names = vec!["GREEN side flow".to_string(), "YELLOW side flow".to_string(), "Press delta".to_string(),"PTU active GREEN".to_string(),"PTU active YELLOW".to_string()];
        let mut ptu_history = History::new(ptu_var_names);

        let green_acc_var_names = vec!["Loop Pressure".to_string(), "Acc gas press".to_string(), "Acc fluid vol".to_string(),"Acc gas vol".to_string()];
        let mut accuGreenHistory = History::new(green_acc_var_names);

        let yellow_acc_var_names = vec!["Loop Pressure".to_string(), "Acc gas press".to_string(), "Acc fluid vol".to_string(),"Acc gas vol".to_string()];
        let mut accuYellowHistory = History::new(yellow_acc_var_names);


        let mut epump = electric_pump();
        epump.stop();
        let mut yellow_loop = hydraulic_loop(LoopColor::Yellow);

        let mut edp1 = engine_driven_pump();
        assert!(!edp1.active); //Is off when created?

        let mut engine1 = engine(Ratio::new::<percent>(0.0));

        let mut green_loop = hydraulic_loop(LoopColor::Green);

        let mut ptu = Ptu::new();

        let ct = context(Duration::from_millis(100));


        LoopHistory.init(0.0, vec![green_loop.loop_pressure.get::<psi>(), yellow_loop.loop_pressure.get::<psi>(),green_loop.reservoir_volume.get::<gallon>(), yellow_loop.reservoir_volume.get::<gallon>(), green_loop.current_delta_vol.get::<gallon>(),yellow_loop.current_delta_vol.get::<gallon>()]) ;
        ptu_history.init(0.0,vec![ptu.flow_to_left.get::<gallon_per_second>(), ptu.flow_to_right.get::<gallon_per_second>(),green_loop.loop_pressure.get::<psi>()-yellow_loop.loop_pressure.get::<psi>(),ptu.isActiveLeft as i8 as f64, ptu.isActiveRight as i8 as f64 ]);
        accuGreenHistory.init(0.0,vec![green_loop.loop_pressure.get::<psi>(), green_loop.accumulator_gas_pressure.get::<psi>() ,green_loop.accumulator_fluid_volume.get::<gallon>(),green_loop.accumulator_gas_volume.get::<gallon>()]);
        accuYellowHistory.init(0.0,vec![yellow_loop.loop_pressure.get::<psi>(), yellow_loop.accumulator_gas_pressure.get::<psi>() ,yellow_loop.accumulator_fluid_volume.get::<gallon>(),yellow_loop.accumulator_gas_volume.get::<gallon>()]);

        let yellow_res_at_start = yellow_loop.reservoir_volume;
        let green_res_at_start = green_loop.reservoir_volume;

        engine1.n2=Ratio::new::<percent>(100.0);
        for x in 0..800 {

            if x == 10 { //After 1s powering electric pump
                println!("------------YELLOW EPUMP ON------------");
                assert!(yellow_loop.loop_pressure <= Pressure::new::<psi>(50.0));
                assert!(yellow_loop.reservoir_volume == yellow_res_at_start);

                assert!(green_loop.loop_pressure <= Pressure::new::<psi>(50.0));
                assert!(green_loop.reservoir_volume  == green_res_at_start);

                epump.start();
            }

            if x == 110 { //10s later enabling ptu
                println!("--------------PTU ENABLED--------------");
                assert!(yellow_loop.loop_pressure >= Pressure::new::<psi>(2950.0));
                assert!(yellow_loop.reservoir_volume <= yellow_res_at_start);

                assert!(green_loop.loop_pressure <= Pressure::new::<psi>(50.0));
                assert!(green_loop.reservoir_volume  == green_res_at_start);

                ptu.enabling(true);
            }

            if x == 300 { //@30s, ptu should be supplying green loop
                println!("----------PTU SUPPLIES GREEN------------");
                assert!(yellow_loop.loop_pressure >= Pressure::new::<psi>(2400.0));
                assert!(green_loop.loop_pressure >= Pressure::new::<psi>(2400.0));
            }

            if x == 400 { //@40s enabling edp
                println!("------------GREEN  EDP1  ON------------");
               assert!(yellow_loop.loop_pressure >= Pressure::new::<psi>(2600.0));
               assert!(green_loop.loop_pressure >= Pressure::new::<psi>(2000.0));
                edp1.start();
            }

            if x >= 500 && x <= 600{ //10s later and during 10s, ptu should stay inactive
                println!("------------IS PTU ACTIVE??------------");
               assert!(yellow_loop.loop_pressure >= Pressure::new::<psi>(2900.0));
               assert!(green_loop.loop_pressure >= Pressure::new::<psi>(2900.0));
               assert!( !ptu.isActiveLeft && !ptu.isActiveRight );
            }

            if x == 600 { //@60s diabling edp and epump
                println!("-------------ALL PUMPS OFF------------");
               assert!(yellow_loop.loop_pressure >= Pressure::new::<psi>(2900.0));
               assert!(green_loop.loop_pressure >= Pressure::new::<psi>(2900.0));
                edp1.stop();
                // epump.active = false;
            }

            if x == 800 { //@80s diabling edp and epump
                println!("-----------IS PRESSURE OFF?-----------");
               assert!(yellow_loop.loop_pressure < Pressure::new::<psi>(50.0));
               assert!(green_loop.loop_pressure >= Pressure::new::<psi>(50.0));

               assert!(green_loop.reservoir_volume  > Volume::new::<gallon>(0.0) && green_loop.reservoir_volume  <= green_res_at_start);
               assert!(yellow_loop.reservoir_volume  > Volume::new::<gallon>(0.0) && yellow_loop.reservoir_volume  <= yellow_res_at_start);
            }

            ptu.update(&green_loop, &yellow_loop);
            edp1.update(&ct.delta,&ct, &green_loop, &engine1);
            epump.update(&ct.delta,&ct, &yellow_loop);

            yellow_loop.update(&ct.delta,&ct, vec![&epump], Vec::new(), Vec::new(), vec![&ptu]);
            green_loop.update(&ct.delta,&ct, Vec::new(), vec![&edp1], Vec::new(), vec![&ptu]);

            LoopHistory.update( ct.delta.as_secs_f64(),vec![green_loop.loop_pressure.get::<psi>(), yellow_loop.loop_pressure.get::<psi>(),green_loop.reservoir_volume.get::<gallon>(), yellow_loop.reservoir_volume.get::<gallon>(), green_loop.current_delta_vol.get::<gallon>(),yellow_loop.current_delta_vol.get::<gallon>()]) ;
            ptu_history.update(ct.delta.as_secs_f64(),vec![ptu.flow_to_left.get::<gallon_per_second>(), ptu.flow_to_right.get::<gallon_per_second>(),green_loop.loop_pressure.get::<psi>()-yellow_loop.loop_pressure.get::<psi>(),ptu.isActiveLeft as i8 as f64, ptu.isActiveRight as i8 as f64 ]);

            accuGreenHistory.update(ct.delta.as_secs_f64(),vec![green_loop.loop_pressure.get::<psi>(), green_loop.accumulator_gas_pressure.get::<psi>() ,green_loop.accumulator_fluid_volume.get::<gallon>(),green_loop.accumulator_gas_volume.get::<gallon>()]);
            accuYellowHistory.update(ct.delta.as_secs_f64(),vec![yellow_loop.loop_pressure.get::<psi>(), yellow_loop.accumulator_gas_pressure.get::<psi>() ,yellow_loop.accumulator_fluid_volume.get::<gallon>(),yellow_loop.accumulator_gas_volume.get::<gallon>()]);

            if x % 20 == 0 {
                println!("Iteration {}", x);
                println!("-------------------------------------------");
                println!("---PSI YELLOW: {}", yellow_loop.loop_pressure.get::<psi>());
                println!("---RPM YELLOW: {}", epump.rpm);
                println!("---Priming State: {}/{}", yellow_loop.loop_volume.get::<gallon>(),yellow_loop.max_loop_volume.get::<gallon>());
                println!("---PSI GREEN: {}", green_loop.loop_pressure.get::<psi>());
                println!("---N2  GREEN: {}", engine1.n2.get::<percent>() );
                println!("---Priming State: {}/{}", green_loop.loop_volume.get::<gallon>(),green_loop.max_loop_volume.get::<gallon>());


            }
        }

        LoopHistory.showMatplotlib("yellow_green_ptu_loop_simulation()_Loop_press");
        ptu_history.showMatplotlib("yellow_green_ptu_loop_simulation()_PTU");

        accuGreenHistory.showMatplotlib("yellow_green_ptu_loop_simulation()_Green_acc");
        accuYellowHistory.showMatplotlib("yellow_green_ptu_loop_simulation()_Yellow_acc");

        assert!(true)
    }


    fn hydraulic_loop(loop_color: LoopColor) -> HydLoop {
        match loop_color {
        LoopColor::Yellow => HydLoop::new(
                loop_color,
                false,
                true,
                Volume::new::<gallon>(26.00),
                Volume::new::<gallon>(26.41),
                Volume::new::<gallon>(10.0),
                Volume::new::<gallon>(3.83),
                HydFluid::new(Pressure::new::<pascal>(1450000000.0))
            ),
        LoopColor::Green => HydLoop::new(
                loop_color,
                true,
                false,
                Volume::new::<gallon>(10.2),
                Volume::new::<gallon>(10.2),
                Volume::new::<gallon>(8.0),
                Volume::new::<gallon>(3.3),
                HydFluid::new(Pressure::new::<pascal>(1450000000.0))
            ),
        _ => HydLoop::new(
                loop_color,
                false,
                false,
                Volume::new::<gallon>(15.85),
                Volume::new::<gallon>(15.85),
                Volume::new::<gallon>(8.0),
                Volume::new::<gallon>(1.5),
                HydFluid::new(Pressure::new::<pascal>(1450000000.0)),
            )
        }
    }

    fn electric_pump() -> ElectricPump {
        ElectricPump::new()
    }

    fn engine_driven_pump() -> EngineDrivenPump {
        EngineDrivenPump::new()
    }

    fn engine(n2: Ratio) -> Engine {
        let mut engine = Engine::new(1);
        engine.n2 = n2;

        engine
    }

    fn context(delta_time: Duration) -> UpdateContext {
        UpdateContext::new(
            delta_time,
            Velocity::new::<knot>(250.),
            Length::new::<foot>(5000.),
            ThermodynamicTemperature::new::<degree_celsius>(25.0),
            true,
        )
    }

    #[cfg(test)]

    struct PressureCaracteristic {
        pressure: Pressure,
        rpmTab : Vec <f64>,
        flowTab : Vec <f64>,
    }

    mod characteristics_tests {
        use super::*;

        fn show_carac(figure_title : &str, outputCaracteristics : & Vec<PressureCaracteristic>){
            use rustplotlib::{Axes2D, Line2D};

            let mut allAxis: Vec<Option<Axes2D>> = Vec::new();
            let colors = ["blue", "yellow" ,"red" ,"black","cyan","magenta","green"];
            let linestyles = ["--" , "-.", "-"];
            let mut currAxis = Axes2D::new();
            currAxis=currAxis.grid(true);
            let mut colorIdx=0;
            let mut styleIdx=0;
            for curPressure in outputCaracteristics {
                let press_str = format!("P={:.0}", curPressure.pressure.get::<psi>());
                currAxis=currAxis.add(Line2D::new(press_str.as_str())
                    .data(&curPressure.rpmTab, &curPressure.flowTab)
                    .color(colors[colorIdx])
                    //.marker("x")
                    .linestyle(linestyles[styleIdx])
                    .linewidth(1.0))
                    .xlabel("RPM")
                    .ylabel("Max Flow")
                    .legend("best")
                    .xlim(0.0, *curPressure.rpmTab.last().unwrap());
                    //.ylim(-2.0, 2.0);
                   colorIdx=(colorIdx+1)%colors.len();
                   styleIdx=(styleIdx+1)%linestyles.len();

            }
            allAxis.push(Some(currAxis));
            let fig = Figure::new()
            .subplots(allAxis.len() as u32, 1, allAxis);

            use rustplotlib::Backend;
            use rustplotlib::backend::Matplotlib;
            let mut mpl = Matplotlib::new().unwrap();
            mpl.set_style("ggplot").unwrap();

            fig.apply(&mut mpl).unwrap();


            mpl.savefig(figure_title);

            mpl.wait().unwrap();
        }

        #[test]
        fn epump_charac(){
            let mut outputCaracteristics : Vec<PressureCaracteristic> = Vec::new();
            let mut epump = ElectricPump::new();
            let context = context(Duration::from_secs_f64(0.0001) ); //Small dt to freeze spool up effect

            let mut green_loop = hydraulic_loop(LoopColor::Green);

            epump.start();
            for pressure in (0..3500).step_by(500) {
                let mut rpmTab: Vec<f64> = Vec::new();
                let mut flowTab: Vec<f64> = Vec::new();
                for rpm in (0..10000).step_by(150) {
                    green_loop.loop_pressure=Pressure::new::<psi>(pressure as f64);
                    epump.rpm=rpm as f64;
                    epump.update(&context.delta, &context, &green_loop);
                    rpmTab.push(rpm as f64);
                    let flow=epump.get_delta_vol_max()/ Time::new::<second>(context.delta.as_secs_f64());
                    let flowGal = flow.get::<gallon_per_second>() as f64;
                    flowTab.push(flowGal);
                }
                outputCaracteristics.push(PressureCaracteristic{pressure:green_loop.loop_pressure,rpmTab,flowTab});
            }
            show_carac("Epump_carac",&outputCaracteristics);
        }

        #[test]
        //TODO broken until rpm relation repaired
        fn engine_d_pump_charac(){
            let mut outputCaracteristics : Vec<PressureCaracteristic> = Vec::new();
            let mut edpump = EngineDrivenPump::new();
            //let context = context(Duration::from_secs_f64(0.0001) ); //Small dt to freeze spool up effect

            let mut green_loop = hydraulic_loop(LoopColor::Green);
            let mut engine1 = engine(Ratio::new::<percent>(0.0));

            edpump.start();
            let context = context(Duration::from_secs_f64(1.0) ); //Small dt to freeze spool up effect

            edpump.update(&context.delta, &context, &green_loop,&engine1);
            for pressure in (0..3500).step_by(500) {
                let mut rpmTab: Vec<f64> = Vec::new();
                let mut flowTab: Vec<f64> = Vec::new();
                for rpm in (0..10000).step_by(150) {
                    green_loop.loop_pressure=Pressure::new::<psi>(pressure as f64);
                    engine1.n2=Ratio::new::<percent>((rpm as f64)/(4.0*EngineDrivenPump::MAX_RPM));
                    edpump.update(&context.delta, &context, &green_loop,&engine1);
                    rpmTab.push(rpm as f64);
                    let flow=edpump.get_delta_vol_max()/ Time::new::<second>(context.delta.as_secs_f64());
                    let flowGal = flow.get::<gallon_per_second>() as f64;
                    flowTab.push(flowGal);
                }
                outputCaracteristics.push(PressureCaracteristic{pressure:green_loop.loop_pressure,rpmTab,flowTab});
            }
            show_carac("Eng_Driv_pump_carac",&outputCaracteristics);
        }


    }

    #[cfg(test)]
    mod utility_tests {
        use crate::hydraulic::interpolation;
        use rand::Rng;
        use std::time::{Duration,Instant};

        #[test]
        fn interp_test(){
            let xs1 =  [-100.0, -10.0, 10.0, 240.0, 320.0, 435.3, 678.9, 890.3, 10005.0, 203493.7];
            let ys1 =  [-200.0, 10.0, 40.0, -553.0, 238.4, 30423.3, 23000.2, 32000.4, 43200.2,34.2];

            //Check before first element
            assert!(interpolation(&xs1, &ys1, -500.0)==ys1[0]);

            //Check after last
            assert!(interpolation(&xs1, &ys1, 100000000.0)==*ys1.last().unwrap());

            //Check equal first
            assert!(interpolation(&xs1, &ys1, *xs1.first().unwrap())==*ys1.first().unwrap());

            //Check equal last
            assert!(interpolation(&xs1, &ys1, *xs1.last().unwrap())==*ys1.last().unwrap());

            //Check interp middle
            let res=interpolation(&xs1, &ys1, 358.0);
            assert!((res-10186.589).abs() < 0.001 );

            //Check interp last segment
            let res=interpolation(&xs1, &ys1, 22200.0);
            assert!((res-40479.579).abs() < 0.001 );

            //Check interp first segment
            let res=interpolation(&xs1, &ys1, -50.0);
            assert!((res-(-83.3333)).abs() < 0.001 );

            //Speed check
            let mut rng = rand::thread_rng();
            let timeStart = Instant::now();
            for idx in 0..1000000 {
                let testVal= rng.gen_range(xs1[0]..*xs1.last().unwrap());
                let mut res=interpolation(&xs1, &ys1, testVal);
                res=res+2.78;
            }
            let time_elapsed = timeStart.elapsed();

            println!(
                "Time elapsed for 1000000 calls {} s",
                time_elapsed.as_secs_f64()
            );

            //assert!(time_elapsed < Duration::from_millis(1500) );
        }

    }
    #[cfg(test)]
    mod loop_tests {}

    #[cfg(test)]
    mod epump_tests {}

    //TODO to update according to new caracteristics, spoolup times and displacement dynamic
    // #[cfg(test)]
    // mod edp_tests {
    //     use super::*;
    //     use uom::si::ratio::percent;

    //     #[test]
    //     fn starts_inactive() {
    //         assert!(engine_driven_pump().active == false);
    //     }

    //     #[test]
    //     fn max_flow_under_2500_psi_after_100ms() {
    //         let n2 = Ratio::new::<percent>(60.0);
    //         let pressure = Pressure::new::<psi>(2000.);
    //         let time = Duration::from_millis(100);
    //         let displacement = Volume::new::<cubic_inch>(EngineDrivenPump::DISPLACEMENT_MAP.iter().cloned().fold(-1./0. /* -inf */, f64::max));
    //         assert!(delta_vol_equality_check(n2, displacement, pressure, time))
    //     }

    //     #[test]
    //     fn zero_flow_above_3000_psi_after_25ms() {
    //         let n2 = Ratio::new::<percent>(60.0);
    //         let pressure = Pressure::new::<psi>(3100.);
    //         let time = Duration::from_millis(25);
    //         let displacement = Volume::new::<cubic_inch>(0.);
    //         assert!(delta_vol_equality_check(n2, displacement, pressure, time))
    //     }

    //     fn delta_vol_equality_check(
    //         n2: Ratio,
    //         displacement: Volume,
    //         pressure: Pressure,
    //         time: Duration,
    //     ) -> bool {
    //         let actual = get_edp_actual_delta_vol_when(n2, pressure, time);
    //         let predicted = get_edp_predicted_delta_vol_when(n2, displacement, time);
    //         println!("Actual: {}", actual.get::<gallon>());
    //         println!("Predicted: {}", predicted.get::<gallon>());
    //         actual == predicted
    //     }

    //     fn get_edp_actual_delta_vol_when(n2: Ratio, pressure: Pressure, time: Duration) -> Volume {
    //         let eng = engine(n2);
    //         let mut edp = engine_driven_pump();
    //         let mut line = hydraulic_loop(LoopColor::Green);
    //         let mut context = context((time));
    //         line.loop_pressure = pressure;
    //         edp.update(&time,&context, &line, &eng);
    //         edp.get_delta_vol_max()
    //     }

    //     fn get_edp_predicted_delta_vol_when(
    //         n2: Ratio,
    //         displacement: Volume,
    //         time: Duration,
    //     ) -> Volume {
    //         let edp_rpm = (1.0f64.min(4.0 * n2.get::<percent>())) * EngineDrivenPump::MAX_RPM;
    //         let expected_flow = Pump::calculate_flow(edp_rpm, displacement);
    //         expected_flow * Time::new::<second>(time.as_secs_f64())
    //     }
    // }
}
