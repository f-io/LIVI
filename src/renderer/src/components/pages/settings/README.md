# ⚙️ Settings System — Developer Guide

This module implements a **declarative settings system** using a schema.
Navigation, pages, lists, and fields are automatically generated from a JSON-like structure.

---

## 🧠 Core Idea

- **All settings are described in a schema**
- No need to manually write pages or routes
- The same element:
  - appears in the list
  - has a dedicated edit page
- Nested sections are supported (any depth)

---

## 📦 Main Entities

### SettingsNode

Each schema element is a `SettingsNode`.

It can be:

- a section (`route`)
- a field (`string`, `number`, `checkbox`, `slider`, `select`, `color`)
- a custom page (`custom`)

---

## 🗂 Root Schema

```ts
export const settingsSchema: SettingsNode = {
  type: 'route',
  route: 'new-settings',
  label: 'Settings',
  path: 'settings',
  children: [generalSchema, audioSchema, videoSchema, appearanceSchema]
}
```

- route — URL segment
- label — displayed name
- children — nested sections or fields

From this schema, the system automatically:

- generates routes (generateRoutes)
- defines navigation
- renders pages

### Navigation

```ts
export const settingsRoutes = generateRoutes(settingsSchema)
```

## ❗ Do not manually add routes

### Everything should go through the schema

## Node Types

| Type       | Description         | UI in list              | UI on page             |
| ---------- | ------------------- | ----------------------- | ---------------------- |
| `route`    | Section / container | Menu item with arrow    | List of children       |
| `string`   | Text field          | Label + arrow if `page` | Text input             |
| `number`   | Number field        | Label + arrow if `page` | Number input / spinner |
| `checkbox` | Boolean             | Switch                  | Switch                 |
| `slider`   | Float 0-1           | Slider in list          | Slider                 |
| `select`   | Dropdown            | Label + arrow if `page` | Select / dropdown      |
| `color`    | Color picker        | Color preview           | Input or picker        |
| `custom`   | Custom component    | Rendered as-is          | Rendered as-is         |

Items with page property always open a dedicated page on click.

## Example Nodes

```ts
{
  type: 'route',
  route: 'general',
  label: 'General',
  children: []
}
```

- creates a menu item
- can contain other sections or fields

### Simple Field

```ts
{
  type: 'checkbox',
  label: 'Fullscreen',
  path: 'kiosk'
}
```

- path — path in the store (settings.kiosk)
- automatically rendered in the list
- value synchronized with the store

### Field with dedicated page

```ts
{
  type: 'string',
  label: 'Car name',
  path: 'carName',
  page: {
    title: 'Car name',
    description: 'Displayed vehicle name'
  }
}
```

Behavior:

- in list → shows as an item with arrow
- on click → opens dedicated page
- editable via universal control

### Select (dropdown)

```ts
{
  type: 'select',
  label: 'Mic type',
  path: 'micType',
  options: [
    { label: 'Internal', value: 'internal' },
    { label: 'External', value: 'external' }
  ],
  page: {
    title: 'Mic type'
  }
}
```

### Custom Page (custom)

Use only if standard fields are not enough.

```ts
{
  path: 'about',
  type: 'custom',
  label: 'About',
  component: About
}
```

- component — React component
- fully controls its own UI
- does NOT participate in automatic syncing

## Nested Sections

#### Nesting is unlimited:

```ts
{
  type: 'route',
  route: 'connections',
  label: 'Connections',
  children: [
    {
      type: 'route',
      route: 'wifi',
      label: 'Wi-Fi',
      children: [
        {
          type: 'number',
          label: 'Wi-Fi channel',
          path: 'wifiChannel'
        }
      ]
    }
  ]
}
```

URL will be:

```ts
;/settings/aeeglnr / connections / wifi
```

## Adding a New Setting

#### Add a simple field

```ts
{
  type: 'number',
  label: 'FPS',
  path: 'fps',
  page: {
    title: 'FPS',
    description: 'Frames per second'
  }
}
```

- appears in the list
- opens a dedicated page
- value is saved in the store

## Move a setting

#### Just move the object in the schema:

```ts
;-generalSchema.children.push(fpsNode) + videoSchema.children.push(fpsNode)
```

#### No other code changes needed

## Add a new section

```ts
{
  type: 'route',
  route: 'network',
  label: 'Network',
  children: []
}
```

# How state works

- All values come from useCarplayStore
- Local state:
- - initialized from the schema
- - tracks dirty state
- Save button is active only if there are changes

# When to use custom

### Use custom only if:

- complex layout is needed
- graphs / lists / preview
- non-standard behavior

In 90% of cases, standard types are enough:

- string
- number
- checkbox
- select
- slider
- color

# Summary

- Schema = source of truth
- 🧭 Navigation generated automatically
- ♻ Reusable UI controls
- 🧱 Highly scalable
- 🔧 Minimal boilerplate

# Always look at existing schemas — they are the best example of how everything works.
