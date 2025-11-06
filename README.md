<div align="center">
<h2>P4-TAS: P4-based Time-Aware Shaper for Time-Sensitive Networking</h2>

![image](https://img.shields.io/badge/licence-Apache%202.0-blue) ![image](https://img.shields.io/badge/lang-rust-darkred) ![image](https://img.shields.io/badge/built%20with-P4-orange) ![image](https://img.shields.io/badge/v-1.0-yellow) [![Controller build](https://github.com/uni-tue-kn/P4-TAS/actions/workflows/controller.yml/badge.svg)](https://github.com/uni-tue-kn/P4-TAS/actions/workflows/controller.yml) [![Data Plane Build](https://github.com/uni-tue-kn/P4-TAS/actions/workflows/build_data_plane.yml/badge.svg)](https://github.com/uni-tue-kn/P4-TAS/actions/workflows/build_data_plane.yml) ![citation-ready](https://img.shields.io/badge/cite-this%20work-000000)

</div>

## Overview
P4-TAS implements the Time-Aware Shaper (TAS, IEEE 802.1Qbv) and Per-Stream Filtering and Policing (PSFP, IEEE 802.1Qci) on programmable switch ASICs (Intel/Barefoot Tofino™ 2).
The data plane is written in P4, while a Rust-based control plane configures gate schedules, streams, and policing rules.
This platform allows time-based queue gating and per-stream admission control at line rate, and, unlike commercial TSN switches, exposes internal timing behavior, making it useful for experiments and research on TSN/DetNet scheduling accuracy.

### Features
- TAS (tGCL) and PSFP (sGCL) implementation in P4
- Nanosecond-resolution gate scheduling
- DetNet integration using a MPLS/TSN translation layer
- Line-rate forwarding (tested up to 400 Gb/s per port on Tofino2)
- Rust control plane for configuration and runtime management
- JSON-based configuration for streams, sGCLs, tGCLs, and policing parameters

## Installation & Start Instructions

### Data Plane

Go to `implementation` and compile P4-TAS via `make compile TARGET=tofino2`.
This compiles the program and copies the resulting configs to the target directory.

Afterwards, start P4-TAS via `make start TARGET=tofino2`.

This requires a fully setup [SDE](https://github.com/p4lang/open-p4studio) with set `$SDE` and `$SDE_INSTALL` environment variables.

### Control Plane

The controller is written in Rust and can be started via `cd implementation/controller && cargo run`. This will build and start the control plane.

### Configuration

Parameters for PSFP (streams, stream handles, stream gates, flow meters, sGCLs) and TAS (tGCLs) are configured using a json file. See [configuration.json](implementation/controller/configuration.json) for an example.

## Citation
If you use P4-TAS in academic work, please cite:
```
tba
```