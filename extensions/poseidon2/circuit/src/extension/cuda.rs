//! GPU (`GpuBabyBearPoseidon2Engine`) prover extension and VM builder for the Poseidon2 extension.
//!
//! This is the CUDA counterpart of [`super::Poseidon2CpuProverExt`] / [`super::Poseidon2Rv32CpuBuilder`].

use std::sync::Arc;

use openvm_circuit::{
    arch::{
        AirInventory, ChipInventory, ChipInventoryError, DenseRecordArena, VmBuilder,
        VmChipComplex, VmProverExtension,
    },
    system::cuda::{
        extensions::{get_inventory_range_checker, get_or_create_bitwise_op_lookup, SystemGpuBuilder},
        SystemChipInventoryGPU,
    },
};
use openvm_cuda_backend::{BabyBearPoseidon2GpuEngine as GpuBabyBearPoseidon2Engine, GpuBackend};
use openvm_rv32im_circuit::Rv32ImGpuProverExt;
use openvm_poseidon2_air::p3_baby_bear::BabyBear;
use openvm_stark_backend::StarkEngine;
use openvm_stark_sdk::config::baby_bear_poseidon2::BabyBearPoseidon2Config;

use super::*;
use crate::{
    cuda::{Poseidon2PeripheryChipGpu, Poseidon2PermuteChipGpu},
    periphery::Poseidon2PeripheryAir,
    permute::Poseidon2PermuteAir,
};

/// GPU (`Icicle`-free, `openvm-cuda-backend`) proving extension for the Poseidon2 circuit.
pub struct Poseidon2GpuProverExt;

impl VmProverExtension<GpuBabyBearPoseidon2Engine, DenseRecordArena, Poseidon2>
    for Poseidon2GpuProverExt
{
    fn extend_prover(
        &self,
        _: &Poseidon2,
        inventory: &mut ChipInventory<BabyBearPoseidon2Config, DenseRecordArena, GpuBackend>,
    ) -> Result<(), ChipInventoryError> {
        let range_checker = get_inventory_range_checker(inventory);
        let bitwise_lu = get_or_create_bitwise_op_lookup(inventory)?;
        let pointer_max_bits = inventory.airs().pointer_max_bits();
        let timestamp_max_bits = inventory.timestamp_max_bits() as u32;
        let device_ctx = range_checker.device_ctx.clone();

        // WARNING: the periphery chip must be added _before_ the adapter chip so that its tracegen
        // is done _last_: chips are trace-generated in reverse insertion order, and the adapter
        // chip's kernel records the input states the periphery chip then consumes.
        inventory.next_air::<Poseidon2PeripheryAir<BabyBear>>()?;
        let shared_records = Default::default();
        let periphery =
            Poseidon2PeripheryChipGpu::new(shared_records.clone(), device_ctx);
        inventory.add_periphery_chip(Arc::new(periphery));

        inventory.next_air::<Poseidon2PermuteAir>()?;
        let op_chip = Poseidon2PermuteChipGpu::new(
            range_checker,
            bitwise_lu,
            pointer_max_bits,
            timestamp_max_bits,
            shared_records,
        );
        inventory.add_executor_chip(op_chip);

        Ok(())
    }
}

/// GPU (`GpuBabyBearPoseidon2Engine`) builder for [`Poseidon2Rv32Config`].
pub struct Poseidon2Rv32GpuBuilder;

impl VmBuilder<GpuBabyBearPoseidon2Engine> for Poseidon2Rv32GpuBuilder {
    type VmConfig = Poseidon2Rv32Config;
    type SystemChipInventory = SystemChipInventoryGPU;
    type RecordArena = DenseRecordArena;

    fn create_chip_complex(
        &self,
        config: &Poseidon2Rv32Config,
        circuit: AirInventory<BabyBearPoseidon2Config>,
        device_ctx: &openvm_stark_backend::EngineDeviceCtx<GpuBabyBearPoseidon2Engine>,
    ) -> Result<
        VmChipComplex<
            BabyBearPoseidon2Config,
            DenseRecordArena,
            GpuBackend,
            SystemChipInventoryGPU,
        >,
        ChipInventoryError,
    > {
        let mut chip_complex = VmBuilder::<GpuBabyBearPoseidon2Engine>::create_chip_complex(
            &SystemGpuBuilder,
            &config.system,
            circuit,
            device_ctx,
        )?;
        let inventory = &mut chip_complex.inventory;
        VmProverExtension::<GpuBabyBearPoseidon2Engine, DenseRecordArena, _>::extend_prover(
            &Rv32ImGpuProverExt,
            &config.rv32i,
            inventory,
        )?;
        VmProverExtension::<GpuBabyBearPoseidon2Engine, DenseRecordArena, _>::extend_prover(
            &Rv32ImGpuProverExt,
            &config.rv32m,
            inventory,
        )?;
        VmProverExtension::<GpuBabyBearPoseidon2Engine, DenseRecordArena, _>::extend_prover(
            &Rv32ImGpuProverExt,
            &config.io,
            inventory,
        )?;
        VmProverExtension::<GpuBabyBearPoseidon2Engine, DenseRecordArena, _>::extend_prover(
            &Poseidon2GpuProverExt,
            &config.poseidon2,
            inventory,
        )?;
        Ok(chip_complex)
    }
}