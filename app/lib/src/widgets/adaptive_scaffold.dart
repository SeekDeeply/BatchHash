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
/// Narrow (<720) shows a single page with a bottom NavigationBar. Medium
/// (720-1100) shows a destination list and a detail pane. Wide (>=1100) adds
/// a NavigationRail next to the list and detail panes.
class AdaptiveScaffold extends StatelessWidget {
  const AdaptiveScaffold({
    super.key,
    required this.destinations,
    required this.selectedIndex,
    required this.onDestinationSelected,
  });

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
      appBar: AppBar(title: Text(selected.label)),
      body: narrow
          ? SafeArea(top: false, child: selected.page)
          : _buildSplitBody(selected.page, wide: wide),
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

  Widget _buildSplitBody(Widget page, {required bool wide}) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        if (wide) ...<Widget>[
          NavigationRail(
            key: const ValueKey<String>('adaptive-navigation-rail'),
            selectedIndex: selectedIndex,
            onDestinationSelected: onDestinationSelected,
            labelType: NavigationRailLabelType.none,
            destinations: <NavigationRailDestination>[
              for (final AppDestination destination in destinations)
                NavigationRailDestination(
                  icon: Icon(destination.icon),
                  selectedIcon: Icon(destination.selectedIcon),
                  label: Text(destination.label),
                ),
            ],
          ),
          const VerticalDivider(width: 1),
        ],
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
