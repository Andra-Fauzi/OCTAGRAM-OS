# Included at the end of the top-level `project()` call via CMAKE_PROJECT_INCLUDE.
#
# OCTAGRAM userspace implements the Unix ABI (rustc calls it target_family
# "unix"), but CMAKE_SYSTEM_NAME must stay `Generic` so that CMake keeps
# CMAKE_CROSSCOMPILING true and never tries to execute a target binary.
#
# For a Generic system CMake's CMakeSystemSpecificInitialize does
# `unset(UNIX)`, and no platform module sets it again. LLVM's
# HandleLLVMOptions.cmake then takes its
# `elseif(CMAKE_SYSTEM_NAME STREQUAL "Generic")` branch, sets LLVM_ON_UNIX=0,
# and silently drops every Unix implementation. The build then fails with
# unrelated-looking errors such as
# "class llvm::sys::fs::basic_file_status has no member named 'getSize'".
#
# Setting UNIX here (after that unset, before any project logic reads it) is
# what makes LLVM compile and link its Unix sources for this target.

set(UNIX 1)
