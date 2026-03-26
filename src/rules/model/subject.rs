// This module previously contained the Subject newtype wrapper around PropertyRef,
// along with SubjectInput (ObjectRepresentation), FromSubjectStr, and ToSubjectStr traits.
//
// All of these have been replaced by homie5 0.10.0's native Display/FromStr and
// string-based serde on DeviceRef, NodeRef, and PropertyRef.
//
// Subject → PropertyRef (direct usage)
// SubjectInput → PropertyRef's native serde (string-based)
// ToSubjectStr → Display trait on ref types
// FromSubjectStr → FromStr trait on ref types
