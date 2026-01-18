//! Memory layout calculation for tscl types
//!
//! This module computes memory layouts for structs and arrays, including:
//! - Field offsets with proper alignment
//! - Total size of aggregate types
//! - Stack frame layout for local variables
//!
//! All tscl values use NaN-boxing (64-bit), so most values have uniform size.

use crate::ir::{IrStructDef, IrType};

/// Size of a NaN-boxed value in bytes
pub const VALUE_SIZE: u32 = 8;

/// Alignment of a NaN-boxed value in bytes
pub const VALUE_ALIGN: u32 = 8;

/// Layout information for a single struct field
#[derive(Debug, Clone)]
pub struct FieldLayout {
    /// Field name
    pub name: String,
    /// Byte offset from struct start
    pub offset: u32,
    /// Field type
    pub ty: IrType,
    /// Size in bytes
    pub size: u32,
}

/// Complete layout for a struct type
#[derive(Debug, Clone)]
pub struct StructLayout {
    /// Total size in bytes (including padding)
    pub size: u32,
    /// Required alignment in bytes
    pub align: u32,
    /// Layout for each field
    pub fields: Vec<FieldLayout>,
}

impl StructLayout {
    /// Get field layout by name
    pub fn get_field(&self, name: &str) -> Option<&FieldLayout> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Get field layout by index
    pub fn get_field_by_index(&self, index: usize) -> Option<&FieldLayout> {
        self.fields.get(index)
    }
}

/// Layout for an array type
#[derive(Debug, Clone)]
pub struct ArrayLayout {
    /// Element type
    pub element_ty: IrType,
    /// Size of each element in bytes
    pub element_size: u32,
    /// Alignment of elements
    pub element_align: u32,
    /// Header size (for length, capacity, etc.)
    pub header_size: u32,
}

impl ArrayLayout {
    /// Calculate total size for an array of given length
    pub fn size_for_length(&self, len: u32) -> u32 {
        self.header_size + len * self.element_size
    }

    /// Calculate offset of element at given index
    pub fn element_offset(&self, index: u32) -> u32 {
        self.header_size + index * self.element_size
    }
}

/// Stack frame layout for a function
#[derive(Debug, Clone)]
pub struct FrameLayout {
    /// Total frame size in bytes
    pub size: u32,
    /// Slot offsets for local variables (by slot index)
    pub slots: Vec<u32>,
    /// Spill area offset (for register spills)
    pub spill_offset: u32,
    /// Spill area size
    pub spill_size: u32,
}

impl FrameLayout {
    /// Create a new frame layout with given number of local slots
    pub fn new(num_slots: u32) -> Self {
        let mut slots = Vec::with_capacity(num_slots as usize);
        let mut offset = 0u32;

        for _ in 0..num_slots {
            slots.push(offset);
            offset += VALUE_SIZE;
        }

        // Add spill area after locals
        let spill_offset = offset;
        let spill_size = 16 * VALUE_SIZE; // Reserve space for 16 spills

        Self {
            size: spill_offset + spill_size,
            slots,
            spill_offset,
            spill_size,
        }
    }

    /// Get offset for a local slot
    pub fn slot_offset(&self, slot: u32) -> Option<u32> {
        self.slots.get(slot as usize).copied()
    }
}

/// Get the size in bytes for an IR type
pub fn value_size(ty: &IrType) -> u32 {
    match ty {
        // Primitives: NaN-boxed 64-bit
        IrType::Number | IrType::Boolean | IrType::Any => VALUE_SIZE,

        // References: 64-bit pointer (or NaN-boxed pointer)
        IrType::String | IrType::Object | IrType::Array | IrType::Function => VALUE_SIZE,

        // Typed arrays: same as untyped (pointer to heap)
        IrType::TypedArray(_) => VALUE_SIZE,

        // Struct reference: pointer to heap-allocated struct
        IrType::Struct(_) => VALUE_SIZE,

        // References: 64-bit pointer
        IrType::Ref(_) | IrType::MutRef(_) => VALUE_SIZE,

        // Special types
        IrType::Void => 0,
        IrType::Never => 0,
    }
}

/// Get the alignment in bytes for an IR type
pub fn value_align(ty: &IrType) -> u32 {
    // All values are 8-byte aligned for NaN-boxing compatibility
    match ty {
        IrType::Void | IrType::Never => 1,
        _ => VALUE_ALIGN,
    }
}

/// Compute the memory layout for a struct with given fields
pub fn compute_struct_layout(fields: &[(String, IrType)]) -> StructLayout {
    let mut result_fields = Vec::with_capacity(fields.len());
    let mut offset = 0u32;
    let mut max_align = 1u32;

    for (name, ty) in fields {
        let field_size = value_size(ty);
        let field_align = value_align(ty);

        // Align offset to field alignment
        offset = align_up(offset, field_align);
        max_align = max_align.max(field_align);

        result_fields.push(FieldLayout {
            name: name.clone(),
            offset,
            ty: ty.clone(),
            size: field_size,
        });

        offset += field_size;
    }

    // Align total size to struct alignment
    let size = align_up(offset, max_align);

    StructLayout {
        size,
        align: max_align,
        fields: result_fields,
    }
}

/// Compute the layout for an array type
pub fn compute_array_layout(element_ty: &IrType) -> ArrayLayout {
    let element_size = value_size(element_ty);
    let element_align = value_align(element_ty);

    // Array header: length (u32) + capacity (u32) + pointer to data
    // In our NaN-boxed world, arrays are heap objects with inline header
    let header_size = 16; // ObjectHeader + length + capacity

    ArrayLayout {
        element_ty: element_ty.clone(),
        element_size,
        element_align,
        header_size,
    }
}

/// Align a value up to the given alignment
#[inline]
pub fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}

/// Align a value down to the given alignment
#[inline]
pub fn align_down(value: u32, align: u32) -> u32 {
    value & !(align - 1)
}

/// Compute the memory layout for a struct definition
/// Uses the existing IrStructDef which has pre-computed offsets
pub fn layout_from_struct_def(struct_def: &IrStructDef) -> StructLayout {
    let fields = struct_def
        .fields
        .iter()
        .map(|(name, ty, offset)| FieldLayout {
            name: name.clone(),
            offset: *offset,
            ty: ty.clone(),
            size: value_size(ty),
        })
        .collect();

    StructLayout {
        size: struct_def.size,
        align: struct_def.alignment,
        fields,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::IrStructId;

    #[test]
    fn test_value_sizes() {
        assert_eq!(value_size(&IrType::Number), 8);
        assert_eq!(value_size(&IrType::Boolean), 8);
        assert_eq!(value_size(&IrType::String), 8);
        assert_eq!(value_size(&IrType::Object), 8);
        assert_eq!(value_size(&IrType::Void), 0);
    }

    #[test]
    fn test_frame_layout() {
        let frame = FrameLayout::new(4);
        assert_eq!(frame.slots.len(), 4);
        assert_eq!(frame.slot_offset(0), Some(0));
        assert_eq!(frame.slot_offset(1), Some(8));
        assert_eq!(frame.slot_offset(2), Some(16));
        assert_eq!(frame.slot_offset(3), Some(24));
    }

    #[test]
    fn test_struct_layout_from_def() {
        let mut struct_def = IrStructDef::new(IrStructId(0), "Point".to_string());
        struct_def.add_field("x".to_string(), IrType::Number);
        struct_def.add_field("y".to_string(), IrType::Number);

        let layout = layout_from_struct_def(&struct_def);
        assert_eq!(layout.size, 16);
        assert_eq!(layout.align, 8);
        assert_eq!(layout.fields.len(), 2);

        let x = layout.get_field("x").unwrap();
        assert_eq!(x.offset, 0);

        let y = layout.get_field("y").unwrap();
        assert_eq!(y.offset, 8);
    }

    #[test]
    fn test_array_layout() {
        let layout = compute_array_layout(&IrType::Number);
        assert_eq!(layout.element_size, 8);
        assert_eq!(layout.element_offset(0), 16); // After header
        assert_eq!(layout.element_offset(1), 24);
        assert_eq!(layout.size_for_length(10), 16 + 80); // header + 10 elements
    }

    #[test]
    fn test_align_up() {
        assert_eq!(align_up(0, 8), 0);
        assert_eq!(align_up(1, 8), 8);
        assert_eq!(align_up(7, 8), 8);
        assert_eq!(align_up(8, 8), 8);
        assert_eq!(align_up(9, 8), 16);
    }
}
