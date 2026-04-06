//! Plugins for the Layers router.
//!
//! This module organizes optional plugin components that extend Layers' routing
//! and observability capabilities. Each plugin is a standalone feature that can
//! be integrated into the routing pipeline.
//!
//! # Available Plugins
//!
//! - [`rlef`](rlef) — Runtime Learned Expression Framework. A diversity-enforcing
//!   selection algorithm that tracks per-route "charge" values and uses Coulomb
//!   repulsion to ensure no single route dominates.
//!
//! - [`telemetry`](telemetry) — Integration telemetry. Records structured metrics
//!   after each routing decision and produces health reports.

#![allow(dead_code)]
//! # Available Plugins
//!
//! - [`rlef`](rlef) — Runtime Learned Expression Framework. A diversity-enforcing
//!   selection algorithm that tracks per-route "charge" values and uses Coulomb
//!   repulsion to ensure no single route dominates.
//!
//! - [`telemetry`](telemetry) — Integration telemetry. Records structured metrics
//!   after each routing decision and produces health reports.

pub mod rlef;
pub mod telemetry;
