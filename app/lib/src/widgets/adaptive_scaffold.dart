import 'package:flutter/material.dart';

/// One top-level section shown in the adaptive navigation shell.
class AppDestination {
  const AppDestination({
    required this.label,
    required this.icon,
    required this.selectedIcon,
    required this.page,
  });

  final String label;
  final IconData icon;
  final IconData selectedIcon;
  final Widget page;
}

/// Three-breakpoint navigation shell.
///
/// Exactly one navigation control is visible per breakpoint, and the page name
/// is shown once:
///
/// * narrow (<720): a bottom [NavigationBar] plus the app bar title;
/// * medium (720-1100): a destination list next to the detail pane, no app bar;
/// * wide (>=1100): a labelled [NavigationRail] replaces the list, no app bar.
///
/// Previously the wide layout rendered a rail, a list and the page title at
/// once, which showed the same destinations two or three times.
class AdaptiveScaffold extends StatelessWidget {
  const AdaptiveScaffold({
    super.key,
    required this.destinations,
    required this.selectedIndex,
    required this.onDestinationSelected,
    this.banner,
  });

  /// Optional persistent banner shown above the page content.
  final Widget? banner;

  static const double narrowBreakpoint = 720;
  static const double wideBreakpoint = 1100;
  static const double destinationListWidth = 280;

  final List<AppDestination> destinations;
  final int selectedIndex;
  final ValueChanged<int> onDestinationSelected;

  @override
  Widget build(BuildContext context) {
    final double width = MediaQuery.sizeOf(context).width;
    final bool narrow = width < narrowBreakpoint;
    final bool wide = width >= wideBreakpoint;
    final AppDestination selected = destinations[selectedIndex];
    return Scaffold(
      // The app bar only exists on the narrow layout. Wider layouts already show
      // the selected destination in the rail or the list, and every page repeats
      // its own heading, so a third copy of the same name would be noise.
      appBar: narrow ? AppBar(title: Text(selected.label)) : null,
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          if (banner != null) banner!,
          Expanded(
            child: narrow
                ? SafeArea(top: false, child: selected.page)
                : _buildSplitBody(selected.page, wide: wide),
          ),
        ],
      ),
      bottomNavigationBar: narrow
          ? NavigationBar(
              key: const ValueKey<String>('adaptive-navigation-bar'),
              selectedIndex: selectedIndex,
              onDestinationSelected: onDestinationSelected,
              destinations: <NavigationDestination>[
                for (final AppDestination destination in destinations)
                  NavigationDestination(
                    icon: Icon(destination.icon),
                    selectedIcon: Icon(destination.selectedIcon),
                    label: destination.label,
                  ),
              ],
            )
          : null,
    );
  }

  /// Medium and wide layouts: one navigation control, one content pane.
  ///
  /// * wide (>=1100): an extended rail with labels (labels are honest there,
  ///   unlike the standard rail which hides them and overlaps if forced);
  /// * medium (720-1100): a destination list.
  Widget _buildSplitBody(Widget page, {required bool wide}) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        if (wide)
          NavigationRail(
            key: const ValueKey<String>('adaptive-navigation-rail'),
            selectedIndex: selectedIndex,
            onDestinationSelected: onDestinationSelected,
            labelType: NavigationRailLabelType.all,
            extended: false,
            destinations: <NavigationRailDestination>[
              for (final AppDestination destination in destinations)
                NavigationRailDestination(
                  icon: Icon(destination.icon),
                  selectedIcon: Icon(destination.selectedIcon),
                  label: Text(destination.label),
                ),
            ],
          )
        else
          SizedBox(
            width: destinationListWidth,
            child: _DestinationList(
              destinations: destinations,
              selectedIndex: selectedIndex,
              onDestinationSelected: onDestinationSelected,
            ),
          ),
        const VerticalDivider(width: 1),
        Expanded(
          child: SafeArea(
            top: false,
            child: KeyedSubtree(
              key: const ValueKey<String>('adaptive-detail-pane'),
              child: page,
            ),
          ),
        ),
      ],
    );
  }
}

class _DestinationList extends StatelessWidget {
  const _DestinationList({
    required this.destinations,
    required this.selectedIndex,
    required this.onDestinationSelected,
  });

  final List<AppDestination> destinations;
  final int selectedIndex;
  final ValueChanged<int> onDestinationSelected;

  @override
  Widget build(BuildContext context) {
    return ListView(
      key: const ValueKey<String>('adaptive-destination-list'),
      padding: const EdgeInsets.symmetric(vertical: 8, horizontal: 8),
      children: <Widget>[
        for (var index = 0; index < destinations.length; index += 1)
          ListTile(
            key: ValueKey<String>(
              'adaptive-destination-${destinations[index].label}',
            ),
            selected: index == selectedIndex,
            leading: Icon(
              index == selectedIndex
                  ? destinations[index].selectedIcon
                  : destinations[index].icon,
            ),
            title: Text(destinations[index].label),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(8),
            ),
            onTap: () => onDestinationSelected(index),
          ),
      ],
    );
  }
}
