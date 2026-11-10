# The Flutter tooling requires that developers have CMake 3.10 or later
# installed. You should not increase this version, as doing so will cause
# the plugin to fail to compile for some customers of the plugin.
cmake_minimum_required(VERSION 3.10)

project(nixxa_engine_library VERSION 0.0.1 LANGUAGES C)

# deskbunny_engine.{c,h} are copies of ../../../../engine, and nx_clock.{c,h},
# nx_font5x7.h copies of ../../../../firmware/nixxa_esp32 (mobile/tool/sync_engine.sh).
# nx_clock.c is its own translation unit: its static helpers clash with the engine's.
add_library(nixxa_engine SHARED
  "deskbunny_engine.c"
  "nixxa_engine.c"
  "nx_clock.c"
)

set_target_properties(nixxa_engine PROPERTIES
  PUBLIC_HEADER nixxa_engine.h
  OUTPUT_NAME "nixxa_engine"
  C_STANDARD 11
  C_STANDARD_REQUIRED ON
)

target_compile_definitions(nixxa_engine PUBLIC DART_SHARED_LIB)

if (ANDROID)
  # Support Android 15 16k page size
  target_link_options(nixxa_engine PRIVATE "-Wl,-z,max-page-size=16384")
endif()
