// Fixture: a miniature component manifest in the format LLVM's llvm-build
// writes to tools/llvm-config/LibraryDependencies.inc. The dependency graph is
// small enough that check_octagram_llvm_config.sh can assert the exact library
// order llvm-config would print.

  struct AvailableComponent {
    /// The name of the component.
    const char *Name;

    /// The name of the library for this component (or NULL).
    const char *Library;

    /// Whether the component is installed.
    bool IsInstalled;

    /// The list of libraries required when linking this component.
    const char *RequiredLibraries[8];
  } AvailableComponents[8] = {
  { "aggressiveinstcombine", "LLVMAggressiveInstCombine", true, {"analysis", "core", "support"} },
{ "all", nullptr, true, {"core", "analysis", "support", "tablegen", "demangle", "bitreader"} },
{ "analysis", "LLVMAnalysis", true, {"core", "support"} },
{ "bitreader", "LLVMBitReader", true, {"support"} },
{ "core", "LLVMCore", true, {"bitreader", "support"} },
{ "demangle", "LLVMDemangle", true, {} },
{ "support", "LLVMSupport", true, {"demangle"} },
{ "tablegen", "LLVMTableGen", true, {"support"} },
};
