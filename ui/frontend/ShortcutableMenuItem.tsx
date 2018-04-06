import React, { Fragment } from 'react';

import MenuItem from './MenuItem';

interface ShortcutableMenuItemProps extends React.HTMLProps<HTMLButtonElement> {
  name: string;
  shortcut: string;
}

const ShortcutableMenuItem: React.SFC<ShortcutableMenuItemProps> = ({ name, shortcut, children, ...props }) => (
  <MenuItem>
    <button className="shortcut-item" {...props}>
      <div className="shortcut-item__header">
        <span className="shortcut-item__name">{name}</span>
        <kbd className="shortcut-item__shortcut">{shortcut}</kbd>
      </div>
      <span className="shortcut-item__description">{children}</span>
    </button>
  </MenuItem>
);

export default ShortcutableMenuItem;
