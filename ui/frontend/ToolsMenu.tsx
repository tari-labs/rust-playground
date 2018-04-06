import React from 'react';
import { connect } from 'react-redux';

import MenuGroup from './MenuGroup';
import ShortcutableMenuItem from './ShortcutableMenuItem';

import {
  performClippy,
  performFormat,
} from './actions';
import State from './state';

interface ToolsMenuProps {
  clippy: () => any;
  format: () => any;
  close: () => void;
}

const ToolsMenu: React.SFC<ToolsMenuProps> = props => (
  <MenuGroup title="Tools">
    <ShortcutableMenuItem
      name="Rustfmt"
      shortcut="Alt/Opt + F"
      onClick={() => { props.format(); props.close(); }}>
      Format this code with Rustfmt.
    </ShortcutableMenuItem>
    <ShortcutableMenuItem
      name="Clippy"
      shortcut="Alt/Opt + C"
      onClick={() => { props.clippy(); props.close(); }}>
      Catch common mistakes and improve the code using the Clippy linter.
    </ShortcutableMenuItem>
  </MenuGroup>
);

const mapDispatchToProps = ({
  clippy: performClippy,
  format: performFormat,
});

export default connect(undefined, mapDispatchToProps)(ToolsMenu);
