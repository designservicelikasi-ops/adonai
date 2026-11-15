//! AEGIS SIMTERMINAL — TEST ADAPTER.
//!
//! A simulated MT5 terminal + broker used to test the engine end to end and to
//! demonstrate the software without a broker. It is never a real connection:
//! it identifies itself as `TerminalKind::Simulator`, trades the fictitious
//! instrument `EURUSD.sim`, and everything it produces is synthetic.

pub mod broker;
pub mod terminal;

pub use broker::{sim_spec, Fault, SimBroker, SimBrokerConfig, SIM_SYMBOL};
pub use terminal::{spawn, PriceFeed, SimControl, SimExit, SimHandle, SimTerminalConfig};
