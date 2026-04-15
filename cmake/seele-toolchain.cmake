set(CMAKE_SYSTEM_NAME Seele)

# Let CMake resolve Platform/Seele.cmake from the repository rather than
# falling back to the built-in unknown-platform path.
list(PREPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_LIST_DIR}")
