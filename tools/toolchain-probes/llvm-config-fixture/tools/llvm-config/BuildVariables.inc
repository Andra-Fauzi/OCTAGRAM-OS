// Fixture: the subset of LLVM's generated BuildVariables.inc that
// tools/octagram_llvm_config.py reads. Keep the field names identical to the
// generated file; tools/check_octagram_llvm_config.sh asserts on the values.

#define LLVM_SRC_ROOT "/fixture/llvm"
#define LLVM_OBJ_ROOT "/fixture/build"
#define LLVM_CPPFLAGS "-DFIXTURE"
#define LLVM_CFLAGS " -DFIXTURE"
#define LLVM_LDFLAGS ""
#define LLVM_CXXFLAGS "-std=c++17 -fno-rtti"
#define LLVM_BUILDMODE "Release"
#define LLVM_LIBDIR_SUFFIX ""
#define LLVM_INSTALL_INCLUDEDIR "include"
#define LLVM_INSTALL_PACKAGE_DIR "lib/cmake/llvm"
#define LLVM_TARGETS_BUILT "X86"
#define LLVM_SYSTEM_LIBS "-lrt -ldl -lm"
#define LLVM_BUILD_SYSTEM "cmake"
#define LLVM_HAS_RTTI 0
#define LLVM_DYLIB_VERSION "21"
#define LLVM_TOOLS_INSTALL_DIR "bin"
