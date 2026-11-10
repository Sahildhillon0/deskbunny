// Black theme, the same tokens as the desktop app (app/src/app.css).
import 'package:flutter/material.dart';

abstract final class NixxaColors {
  static const bg = Color(0xFF000000);
  static const surface = Color(0xFF0A0A0A);
  static const surface2 = Color(0xFF111111);
  static const surface3 = Color(0xFF171717);
  static const line = Color(0xFF1F1F1F);
  static const line2 = Color(0xFF2A2A2A);
  static const text = Color(0xFFE9ECEE);
  static const text2 = Color(0xFF9BA1A6);
  static const text3 = Color(0xFF5F666B);
  static const accent = Color(0xFFDFF6FF);
  static const accentInk = Color(0xFF0A1418);
  static const accentSoft = Color(0x1ADFF6FF);
  static const accentLine = Color(0x61DFF6FF);
  static const danger = Color(0xFFF2938A);
  static const ok = Color(0xFFA8E6C8);
  static const warn = Color(0xFFF0C987);
  // The OLED itself.
  static const lit = Color(0xFFDFF6FF);
  static const panel = Color(0xFF020304);
}

ThemeData buildNixxaTheme() {
  const scheme = ColorScheme.dark(
    surface: NixxaColors.bg,
    onSurface: NixxaColors.text,
    onSurfaceVariant: NixxaColors.text2,
    primary: NixxaColors.accent,
    onPrimary: NixxaColors.accentInk,
    secondary: NixxaColors.accent,
    onSecondary: NixxaColors.accentInk,
    secondaryContainer: NixxaColors.accentSoft,
    onSecondaryContainer: NixxaColors.accent,
    error: NixxaColors.danger,
    onError: NixxaColors.bg,
    outline: NixxaColors.line2,
    outlineVariant: NixxaColors.line,
    surfaceContainerLowest: NixxaColors.bg,
    surfaceContainerLow: NixxaColors.surface,
    surfaceContainer: NixxaColors.surface,
    surfaceContainerHigh: NixxaColors.surface2,
    surfaceContainerHighest: NixxaColors.surface3,
  );
  const radius = BorderRadius.all(Radius.circular(10));
  return ThemeData(
    useMaterial3: true,
    brightness: Brightness.dark,
    colorScheme: scheme,
    scaffoldBackgroundColor: NixxaColors.bg,
    canvasColor: NixxaColors.bg,
    dividerColor: NixxaColors.line,
    dividerTheme: const DividerThemeData(color: NixxaColors.line, space: 1, thickness: 1),
    appBarTheme: const AppBarTheme(
      backgroundColor: NixxaColors.bg,
      foregroundColor: NixxaColors.text,
      surfaceTintColor: Colors.transparent,
      elevation: 0,
      scrolledUnderElevation: 0,
      centerTitle: false,
      titleTextStyle: TextStyle(fontSize: 18, fontWeight: FontWeight.w600, color: NixxaColors.text),
    ),
    navigationBarTheme: NavigationBarThemeData(
      backgroundColor: NixxaColors.surface,
      surfaceTintColor: Colors.transparent,
      indicatorColor: NixxaColors.accentSoft,
      height: 64,
      labelTextStyle: WidgetStateProperty.resolveWith(
        (s) => TextStyle(fontSize: 12, color: s.contains(WidgetState.selected) ? NixxaColors.text : NixxaColors.text3),
      ),
      iconTheme: WidgetStateProperty.resolveWith(
        (s) => IconThemeData(color: s.contains(WidgetState.selected) ? NixxaColors.accent : NixxaColors.text3),
      ),
    ),
    cardTheme: const CardThemeData(
      color: NixxaColors.surface,
      surfaceTintColor: Colors.transparent,
      elevation: 0,
      margin: EdgeInsets.zero,
      shape: RoundedRectangleBorder(
        borderRadius: radius,
        side: BorderSide(color: NixxaColors.line),
      ),
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(
        backgroundColor: NixxaColors.accent,
        foregroundColor: NixxaColors.accentInk,
        disabledBackgroundColor: NixxaColors.surface3,
        shape: const RoundedRectangleBorder(borderRadius: BorderRadius.all(Radius.circular(6))),
        minimumSize: const Size(0, 44),
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(
        foregroundColor: NixxaColors.text,
        backgroundColor: NixxaColors.surface2,
        side: const BorderSide(color: NixxaColors.line2),
        shape: const RoundedRectangleBorder(borderRadius: BorderRadius.all(Radius.circular(6))),
        minimumSize: const Size(0, 44),
      ),
    ),
    textButtonTheme: TextButtonThemeData(style: TextButton.styleFrom(foregroundColor: NixxaColors.text2)),
    inputDecorationTheme: const InputDecorationTheme(
      filled: true,
      fillColor: NixxaColors.surface2,
      isDense: true,
      border: OutlineInputBorder(
        borderRadius: BorderRadius.all(Radius.circular(6)),
        borderSide: BorderSide(color: NixxaColors.line2),
      ),
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.all(Radius.circular(6)),
        borderSide: BorderSide(color: NixxaColors.line2),
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.all(Radius.circular(6)),
        borderSide: BorderSide(color: NixxaColors.accentLine),
      ),
      labelStyle: TextStyle(color: NixxaColors.text2),
      hintStyle: TextStyle(color: NixxaColors.text3),
    ),
    chipTheme: ChipThemeData(
      backgroundColor: NixxaColors.surface2,
      selectedColor: NixxaColors.accentSoft,
      side: const BorderSide(color: NixxaColors.line2),
      labelStyle: const TextStyle(fontSize: 13, color: NixxaColors.text),
      secondaryLabelStyle: const TextStyle(fontSize: 13, color: NixxaColors.accent),
      checkmarkColor: NixxaColors.accent,
      shape: const RoundedRectangleBorder(borderRadius: BorderRadius.all(Radius.circular(6))),
      padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
    ),
    sliderTheme: const SliderThemeData(
      activeTrackColor: NixxaColors.accent,
      inactiveTrackColor: NixxaColors.line2,
      thumbColor: NixxaColors.accent,
      overlayColor: NixxaColors.accentSoft,
      trackHeight: 3,
    ),
    switchTheme: SwitchThemeData(
      thumbColor: WidgetStateProperty.resolveWith(
        (s) => s.contains(WidgetState.selected) ? NixxaColors.accentInk : NixxaColors.text2,
      ),
      trackColor: WidgetStateProperty.resolveWith(
        (s) => s.contains(WidgetState.selected) ? NixxaColors.accent : NixxaColors.surface3,
      ),
      trackOutlineColor: WidgetStateProperty.resolveWith(
        (s) => s.contains(WidgetState.selected) ? NixxaColors.accent : NixxaColors.line2,
      ),
    ),
    listTileTheme: const ListTileThemeData(iconColor: NixxaColors.text2, textColor: NixxaColors.text),
    bottomSheetTheme: const BottomSheetThemeData(
      backgroundColor: NixxaColors.surface,
      surfaceTintColor: Colors.transparent,
      showDragHandle: true,
      dragHandleColor: NixxaColors.line2,
    ),
    dialogTheme: const DialogThemeData(
      backgroundColor: NixxaColors.surface2,
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.all(Radius.circular(14)),
        side: BorderSide(color: NixxaColors.line2),
      ),
    ),
    snackBarTheme: const SnackBarThemeData(
      backgroundColor: NixxaColors.surface3,
      contentTextStyle: TextStyle(color: NixxaColors.text),
      behavior: SnackBarBehavior.floating,
    ),
    popupMenuTheme: const PopupMenuThemeData(color: NixxaColors.surface2, surfaceTintColor: Colors.transparent),
    progressIndicatorTheme: const ProgressIndicatorThemeData(
      color: NixxaColors.accent,
      linearTrackColor: NixxaColors.line2,
    ),
    segmentedButtonTheme: SegmentedButtonThemeData(
      style: SegmentedButton.styleFrom(
        backgroundColor: NixxaColors.surface2,
        selectedBackgroundColor: NixxaColors.accentSoft,
        selectedForegroundColor: NixxaColors.accent,
        foregroundColor: NixxaColors.text2,
        side: const BorderSide(color: NixxaColors.line2),
      ),
    ),
  );
}
