import type { NaviBag } from '../types'

export type NavLocale = 'en' | 'de' | 'ua'

type Dict = {
  // generic UI strings
  unknown: string
  app: string
  destination: string
  eta: string
  timeRemaining: string
  distanceRemaining: string
  onRoad: string
  maneuver: string

  // status + direction
  statusIdle: string
  statusActive: string
  drivingSideLeft: string
  drivingSideRight: string
  junctionIntersection: string
  junctionRoundabout: string

  // OrderType
  orderContinue: string
  orderTurn: string
  orderExit: string
  orderRoundabout: string
  orderUturn: string
  orderKeepLeft: string
  orderKeepRight: string
  orderUnknown: string

  // ManeuverType
  noTurn: string // 0
  left: string // 1
  right: string // 2
  straight: string // 3
  uTurn: string // 4
  followRoad: string // 5
  enterRoundabout: string // 6
  exitRoundabout: string // 7
  rampOff: string // 8
  rampOn: string // 9
  endOfNavigation: string // 10
  proceedToRoute: string // 11
  arrived: string // 12
  keepLeft: string // 13
  keepRight: string // 14
  enterFerry: string // 15
  exitFerry: string // 16
  changeFerry: string // 17
  uTurnToRoute: string // 18
  roundaboutUTurn: string // 19
  endOfRoadLeft: string // 20
  endOfRoadRight: string // 21
  rampOffLeft: string // 22
  rampOffRight: string // 23
  arrivedLeft: string // 24
  arrivedRight: string // 25
  uTurnWhenPossible: string // 26
  endOfDirections: string // 27
  roundaboutExit: string // 28..46
  sharpLeft: string // 47
  sharpRight: string // 48
  slightLeft: string // 49
  slightRight: string // 50
  changeHighway: string // 51
  changeHighwayLeft: string // 52
  changeHighwayRight: string // 53
}

const DICT: Record<NavLocale, Dict> = {
  en: {
    // generic UI strings
    unknown: 'Unknown',
    app: 'App',
    destination: 'Destination',
    eta: 'ETA',
    timeRemaining: 'Time remaining',
    distanceRemaining: 'Distance remaining',
    onRoad: 'Road',
    maneuver: 'Maneuver',

    // status + direction
    statusIdle: 'Idle',
    statusActive: 'Active',
    drivingSideLeft: 'Left',
    drivingSideRight: 'Right',
    junctionIntersection: 'Intersection',
    junctionRoundabout: 'Roundabout',

    // OrderType
    orderContinue: 'Continue',
    orderTurn: 'Turn',
    orderExit: 'Exit',
    orderRoundabout: 'Roundabout',
    orderUturn: 'U-turn',
    orderKeepLeft: 'Keep left',
    orderKeepRight: 'Keep right',
    orderUnknown: 'Unknown',

    // ManeuverType (Table 15-16)
    noTurn: 'No turn', // 0
    left: 'Turn left', // 1
    right: 'Turn right', // 2
    straight: 'Go straight', // 3
    uTurn: 'Make a U-turn', // 4
    followRoad: 'Continue on the current road', // 5
    enterRoundabout: 'Enter roundabout', // 6
    exitRoundabout: 'Exit roundabout', // 7
    rampOff: 'Exit highway', // 8
    rampOn: 'Merge onto highway', // 9
    endOfNavigation: 'End of navigation', // 10
    proceedToRoute: 'Proceed to the route', // 11
    arrived: 'Arrived', // 12
    keepLeft: 'Keep left', // 13
    keepRight: 'Keep right', // 14
    enterFerry: 'Enter ferry', // 15
    exitFerry: 'Exit ferry', // 16
    changeFerry: 'Change ferry', // 17
    uTurnToRoute: 'Make a U-turn to rejoin the route', // 18
    roundaboutUTurn: 'Use the roundabout to make a U-turn', // 19
    endOfRoadLeft: 'At the end of the road, turn left', // 20
    endOfRoadRight: 'At the end of the road, turn right', // 21
    rampOffLeft: 'Exit highway on the left', // 22
    rampOffRight: 'Exit highway on the right', // 23
    arrivedLeft: 'Arrived (left)', // 24
    arrivedRight: 'Arrived (right)', // 25
    uTurnWhenPossible: 'Make a U-turn when possible', // 26
    endOfDirections: 'End of directions', // 27
    roundaboutExit: 'Roundabout exit', // 28..46
    sharpLeft: 'Sharp left', // 47
    sharpRight: 'Sharp right', // 48
    slightLeft: 'Slight left', // 49
    slightRight: 'Slight right', // 50
    changeHighway: 'Change highway', // 51
    changeHighwayLeft: 'Change highway (left)', // 52
    changeHighwayRight: 'Change highway (right)' // 53
  },

  de: {
    // generic UI strings
    unknown: 'Unbekannt',
    app: 'App',
    destination: 'Ziel',
    eta: 'Ankunft',
    timeRemaining: 'Restzeit',
    distanceRemaining: 'Reststrecke',
    onRoad: 'Straße',
    maneuver: 'Manöver',

    // status + direction
    statusIdle: 'Inaktiv',
    statusActive: 'Aktiv',
    drivingSideLeft: 'Linksverkehr',
    drivingSideRight: 'Rechtsverkehr',
    junctionIntersection: 'Kreuzung',
    junctionRoundabout: 'Kreisverkehr',

    // OrderType
    orderContinue: 'Weiter',
    orderTurn: 'Abbiegen',
    orderExit: 'Ausfahrt',
    orderRoundabout: 'Kreisverkehr',
    orderUturn: 'Wenden',
    orderKeepLeft: 'Links halten',
    orderKeepRight: 'Rechts halten',
    orderUnknown: 'Unbekannt',

    // ManeuverType (Table 15-16)
    noTurn: 'Keine Abbiegung', // 0
    left: 'Links abbiegen', // 1
    right: 'Rechts abbiegen', // 2
    straight: 'Geradeaus', // 3
    uTurn: 'Wenden', // 4
    followRoad: 'Der Straße folgen', // 5
    enterRoundabout: 'In den Kreisverkehr einfahren', // 6
    exitRoundabout: 'Kreisverkehr verlassen', // 7
    rampOff: 'Autobahn verlassen', // 8
    rampOn: 'Auf die Autobahn auffahren', // 9
    endOfNavigation: 'Navigation beendet', // 10
    proceedToRoute: 'Zur Route fahren', // 11
    arrived: 'Angekommen', // 12
    keepLeft: 'Links halten', // 13
    keepRight: 'Rechts halten', // 14
    enterFerry: 'Auf die Fähre fahren', // 15
    exitFerry: 'Fähre verlassen', // 16
    changeFerry: 'Fähre wechseln', // 17
    uTurnToRoute: 'Wenden und zur Route zurück', // 18
    roundaboutUTurn: 'Im Kreisverkehr wenden', // 19
    endOfRoadLeft: 'Am Ende der Straße links abbiegen', // 20
    endOfRoadRight: 'Am Ende der Straße rechts abbiegen', // 21
    rampOffLeft: 'Autobahn links verlassen', // 22
    rampOffRight: 'Autobahn rechts verlassen', // 23
    arrivedLeft: 'Angekommen (links)', // 24
    arrivedRight: 'Angekommen (rechts)', // 25
    uTurnWhenPossible: 'Bei Gelegenheit wenden', // 26
    endOfDirections: 'Zielführung beendet', // 27
    roundaboutExit: 'Ausfahrt', // 28..46
    sharpLeft: 'Scharf links', // 47
    sharpRight: 'Scharf rechts', // 48
    slightLeft: 'Leicht links', // 49
    slightRight: 'Leicht rechts', // 50
    changeHighway: 'Autobahnwechsel', // 51
    changeHighwayLeft: 'Autobahnwechsel (links)', // 52
    changeHighwayRight: 'Autobahnwechsel (rechts)' // 53
  },

  ua: {
    // generic UI strings
    unknown: 'Невідомо',
    app: 'Додаток',
    destination: 'Пункт призначення',
    eta: 'Прибуття',
    timeRemaining: 'Залишилось часу',
    distanceRemaining: 'Залишилось відстані',
    onRoad: 'Дорога',
    maneuver: 'Маневр',

    // status + direction
    statusIdle: 'Неактивно',
    statusActive: 'Активно',
    drivingSideLeft: 'Лівосторонній рух',
    drivingSideRight: 'Правосторонній рух',
    junctionIntersection: 'Перехрестя',
    junctionRoundabout: 'Кільце',

    // OrderType
    orderContinue: 'Продовжуйте',
    orderTurn: 'Поворот',
    orderExit: 'З’їзд',
    orderRoundabout: 'Кільце',
    orderUturn: 'Розворот',
    orderKeepLeft: 'Тримайтесь ліворуч',
    orderKeepRight: 'Тримайтесь праворуч',
    orderUnknown: 'Невідомо',

    // ManeuverType (Table 15-16)
    noTurn: 'Без повороту', // 0
    left: 'Поверніть ліворуч', // 1
    right: 'Поверніть праворуч', // 2
    straight: 'Рухайтесь прямо', // 3
    uTurn: 'Розворот', // 4
    followRoad: 'Продовжуйте цією дорогою', // 5
    enterRoundabout: "В'їдьте на кільце", // 6
    exitRoundabout: "З'їдьте з кільця", // 7
    rampOff: "З'їзд з автомагістралі", // 8
    rampOn: "В'їзд на автомагістраль", // 9
    endOfNavigation: 'Навігацію завершено', // 10
    proceedToRoute: 'Прямуйте до маршруту', // 11
    arrived: 'Прибули', // 12
    keepLeft: 'Тримайтесь ліворуч', // 13
    keepRight: 'Тримайтесь праворуч', // 14
    enterFerry: 'Заїдьте на пором', // 15
    exitFerry: "З'їдьте з порома", // 16
    changeFerry: 'Змініть пором', // 17
    uTurnToRoute: 'Розворот, щоб повернутись на маршрут', // 18
    roundaboutUTurn: 'Виконайте розворот через кільце', // 19
    endOfRoadLeft: 'В кінці дороги поверніть ліворуч', // 20
    endOfRoadRight: 'В кінці дороги поверніть праворуч', // 21
    rampOffLeft: "З'їзд з автомагістралі ліворуч", // 22
    rampOffRight: "З'їзд з автомагістралі праворуч", // 23
    arrivedLeft: 'Прибули (ліворуч)', // 24
    arrivedRight: 'Прибули (праворуч)', // 25
    uTurnWhenPossible: 'Розверніться, коли буде можливо', // 26
    endOfDirections: 'Маршрут завершено', // 27
    roundaboutExit: 'З’їзд', // 28..46
    sharpLeft: 'Різко ліворуч', // 47
    sharpRight: 'Різко праворуч', // 48
    slightLeft: 'Плавно ліворуч', // 49
    slightRight: 'Плавно праворуч', // 50
    changeHighway: 'Зміна автомагістралі', // 51
    changeHighwayLeft: 'Зміна автомагістралі (ліворуч)', // 52
    changeHighwayRight: 'Зміна автомагістралі (праворуч)' // 53
  }
}

function maneuverTypeText(value: unknown, d: Dict): string | undefined {
  const v = typeof value === 'number' ? value : undefined
  if (v == null) return undefined

  switch (v) {
    case 0:
      return d.noTurn
    case 1:
      return d.left
    case 2:
      return d.right
    case 3:
      return d.straight
    case 4:
      return d.uTurn
    case 5:
      return d.followRoad
    case 6:
      return d.enterRoundabout
    case 7:
      return d.exitRoundabout
    case 8:
      return d.rampOff
    case 9:
      return d.rampOn
    case 10:
      return d.endOfNavigation
    case 11:
      return d.proceedToRoute
    case 12:
      return d.arrived
    case 13:
      return d.keepLeft
    case 14:
      return d.keepRight
    case 15:
      return d.enterFerry
    case 16:
      return d.exitFerry
    case 17:
      return d.changeFerry
    case 18:
      return d.uTurnToRoute
    case 19:
      return d.roundaboutUTurn
    case 20:
      return d.endOfRoadLeft
    case 21:
      return d.endOfRoadRight
    case 22:
      return d.rampOffLeft
    case 23:
      return d.rampOffRight
    case 24:
      return d.arrivedLeft
    case 25:
      return d.arrivedRight
    case 26:
      return d.uTurnWhenPossible
    case 27:
      return d.endOfDirections
    case 28:
    case 29:
    case 30:
    case 31:
    case 32:
    case 33:
    case 34:
    case 35:
    case 36:
    case 37:
    case 38:
    case 39:
    case 40:
    case 41:
    case 42:
    case 43:
    case 44:
    case 45:
    case 46:
      return `${d.roundaboutExit} ${v - 27}`
    case 47:
      return d.sharpLeft
    case 48:
      return d.sharpRight
    case 49:
      return d.slightLeft
    case 50:
      return d.slightRight
    case 51:
      return d.changeHighway
    case 52:
      return d.changeHighwayLeft
    case 53:
      return d.changeHighwayRight
    default:
      return undefined
  }
}

function junctionTypeText(value: unknown, d: Dict): string | undefined {
  const v = typeof value === 'number' ? value : undefined
  if (v == null) return undefined

  switch (v) {
    case 0:
      return d.junctionIntersection
    case 1:
      return d.junctionRoundabout
    default:
      return undefined
  }
}

function fmtSecondsToMmSs(totalSeconds: unknown): string | undefined {
  const s = typeof totalSeconds === 'number' ? totalSeconds : undefined
  if (s == null || !Number.isFinite(s) || s < 0) return undefined
  const mins = Math.floor(s / 60)
  const secs = Math.floor(s % 60)
  return `${mins}:${String(secs).padStart(2, '0')}`
}

function fmtMeters(meters: unknown, locale: string): string | undefined {
  const m = typeof meters === 'number' ? meters : undefined
  if (m == null || !Number.isFinite(m) || m < 0) return undefined

  // UI stability:
  // < 1 km  -> meters (e.g. 999 m)
  // 1..10 km -> always 2 decimals (e.g. 1.58 km, 9.00 km)
  // >= 10 km -> always 1 decimal (e.g. 12.3 km)
  if (m >= 1000) {
    const km = m / 1000
    const fractionDigits = km < 10 ? 2 : 1

    return (
      new Intl.NumberFormat(locale, {
        minimumFractionDigits: fractionDigits,
        maximumFractionDigits: fractionDigits
      }).format(km) + ' km'
    )
  }

  return new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(m) + ' m'
}

export function translateNavigation(navi: NaviBag | null | undefined, locale: NavLocale) {
  const d = DICT[locale] ?? DICT.en
  const langTag = locale === 'de' ? 'de-DE' : locale === 'ua' ? 'uk-UA' : 'en-US'

  const obj = (navi ?? {}) as Record<string, unknown>

  const sourceName = typeof obj.NaviAPPName === 'string' ? obj.NaviAPPName : undefined
  const destinationName =
    typeof obj.NaviDestinationName === 'string' ? obj.NaviDestinationName : undefined
  const currentRoadName = typeof obj.NaviRoadName === 'string' ? obj.NaviRoadName : undefined

  const timeRemainingToDestinationText = fmtSecondsToMmSs(obj.NaviTimeToDestination)
  const distanceRemainingDisplayStringText = fmtMeters(obj.NaviDistanceToDestination, langTag)
  const remainDistanceText = fmtMeters(obj.NaviRemainDistance, langTag)

  const maneuverType = typeof obj.NaviManeuverType === 'number' ? obj.NaviManeuverType : undefined
  const maneuverText = maneuverTypeText(maneuverType, d) ?? d.unknown

  const rawTurnSide = typeof obj.NaviTurnSide === 'number' ? obj.NaviTurnSide : undefined
  const drivingSide = rawTurnSide === 0 || rawTurnSide === 1 ? rawTurnSide : undefined
  const drivingSideText =
    drivingSide === 0 ? d.drivingSideRight : drivingSide === 1 ? d.drivingSideLeft : undefined

  const junctionType = typeof obj.NaviJunctionType === 'number' ? obj.NaviJunctionType : undefined
  const junctionText = junctionTypeText(junctionType, d)

  const routeGuidanceState = typeof obj.NaviStatus === 'number' ? obj.NaviStatus : undefined
  const maneuverState = typeof obj.NaviOrderType === 'number' ? obj.NaviOrderType : undefined
  const turnAngle = typeof obj.NaviTurnAngle === 'number' ? obj.NaviTurnAngle : undefined

  return {
    SourceName: sourceName,
    DestinationName: destinationName,
    CurrentRoadName: currentRoadName,

    TimeRemainingToDestinationText: timeRemainingToDestinationText,
    DistanceRemainingDisplayStringText: distanceRemainingDisplayStringText,
    RemainDistanceText: remainDistanceText,

    ManeuverTypeText: maneuverText,
    JunctionTypeText: junctionText,
    DrivingSideText: drivingSideText,

    codes: {
      RouteGuidanceState: routeGuidanceState,
      ManeuverState: maneuverState,
      ManeuverType: maneuverType,
      TurnAngle: turnAngle,
      TurnSide: rawTurnSide,
      DrivingSide: drivingSide,
      JunctionType: junctionType
    }
  }
}
