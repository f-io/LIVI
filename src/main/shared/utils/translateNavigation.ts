import type { NaviBag } from '@shared/types'
import {
  DrivingSide,
  JunctionType,
  ManeuverType,
  roundaboutExitNumber
} from '@shared/types/NavigationTypes'

export type NavLocale = 'en' | 'de' | 'ua' | 'fr'

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
  noRoute: string
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
    noRoute: 'No Route',
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
    noRoute: 'Zielführung aus',
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
    noRoute: 'Немає маршруту',
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
  },

  fr: {
    // generic UI strings
    unknown: 'Inconnu',
    app: 'App',
    destination: 'Destination',
    eta: 'Arrivée',
    timeRemaining: 'Temps restant',
    distanceRemaining: 'Distance restante',
    onRoad: 'Route',
    maneuver: 'Manœuvre',

    // status + direction
    statusIdle: 'Inactif',
    statusActive: 'Actif',
    noRoute: 'Aucun itinéraire',
    drivingSideLeft: 'Conduite à gauche',
    drivingSideRight: 'Conduite à droite',
    junctionIntersection: 'Intersection',
    junctionRoundabout: 'Rond-point',

    // OrderType
    orderContinue: 'Continuez',
    orderTurn: 'Tournez',
    orderExit: 'Sortie',
    orderRoundabout: 'Rond-point',
    orderUturn: 'Demi-tour',
    orderKeepLeft: 'Restez à gauche',
    orderKeepRight: 'Restez à droite',
    orderUnknown: 'Inconnu',

    // ManeuverType (Table 15-16)
    noTurn: 'Pas de virage', // 0
    left: 'Tournez à gauche', // 1
    right: 'Tournez à droite', // 2
    straight: 'Continuez tout droit', // 3
    uTurn: 'Faites demi-tour', // 4
    followRoad: 'Continuez sur la route actuelle', // 5
    enterRoundabout: 'Entrez dans le rond-point', // 6
    exitRoundabout: 'Sortez du rond-point', // 7
    rampOff: "Quittez l'autoroute", // 8
    rampOn: "Rejoignez l'autoroute", // 9
    endOfNavigation: 'Fin de la navigation', // 10
    proceedToRoute: "Rejoignez l'itinéraire", // 11
    arrived: 'Arrivé', // 12
    keepLeft: 'Restez à gauche', // 13
    keepRight: 'Restez à droite', // 14
    enterFerry: 'Embarquez sur le ferry', // 15
    exitFerry: 'Quittez le ferry', // 16
    changeFerry: 'Changez de ferry', // 17
    uTurnToRoute: "Faites demi-tour pour rejoindre l'itinéraire", // 18
    roundaboutUTurn: 'Faites demi-tour au rond-point', // 19
    endOfRoadLeft: 'Au bout de la route, tournez à gauche', // 20
    endOfRoadRight: 'Au bout de la route, tournez à droite', // 21
    rampOffLeft: "Quittez l'autoroute à gauche", // 22
    rampOffRight: "Quittez l'autoroute à droite", // 23
    arrivedLeft: 'Arrivé (à gauche)', // 24
    arrivedRight: 'Arrivé (à droite)', // 25
    uTurnWhenPossible: 'Faites demi-tour dès que possible', // 26
    endOfDirections: 'Fin du guidage', // 27
    roundaboutExit: 'Sortie', // 28..46
    sharpLeft: 'Virage serré à gauche', // 47
    sharpRight: 'Virage serré à droite', // 48
    slightLeft: 'Légèrement à gauche', // 49
    slightRight: 'Légèrement à droite', // 50
    changeHighway: "Changement d'autoroute", // 51
    changeHighwayLeft: "Changement d'autoroute (à gauche)", // 52
    changeHighwayRight: "Changement d'autoroute (à droite)" // 53
  }
}

const MANEUVER_TEXT_KEY: Partial<Record<ManeuverType, keyof Dict>> = {
  [ManeuverType.NoTurn]: 'noTurn',
  [ManeuverType.LeftTurn]: 'left',
  [ManeuverType.RightTurn]: 'right',
  [ManeuverType.Straight]: 'straight',
  [ManeuverType.UTurn]: 'uTurn',
  [ManeuverType.FollowRoad]: 'followRoad',
  [ManeuverType.EnterRoundabout]: 'enterRoundabout',
  [ManeuverType.ExitRoundabout]: 'exitRoundabout',
  [ManeuverType.RampOff]: 'rampOff',
  [ManeuverType.RampOn]: 'rampOn',
  [ManeuverType.EndOfNavigation]: 'endOfNavigation',
  [ManeuverType.ProceedToRoute]: 'proceedToRoute',
  [ManeuverType.Arrived]: 'arrived',
  [ManeuverType.KeepLeft]: 'keepLeft',
  [ManeuverType.KeepRight]: 'keepRight',
  [ManeuverType.EnterFerry]: 'enterFerry',
  [ManeuverType.ExitFerry]: 'exitFerry',
  [ManeuverType.ChangeFerry]: 'changeFerry',
  [ManeuverType.UTurnToRoute]: 'uTurnToRoute',
  [ManeuverType.RoundaboutUTurn]: 'roundaboutUTurn',
  [ManeuverType.EndOfRoadLeft]: 'endOfRoadLeft',
  [ManeuverType.EndOfRoadRight]: 'endOfRoadRight',
  [ManeuverType.RampOffLeft]: 'rampOffLeft',
  [ManeuverType.RampOffRight]: 'rampOffRight',
  [ManeuverType.ArrivedLeft]: 'arrivedLeft',
  [ManeuverType.ArrivedRight]: 'arrivedRight',
  [ManeuverType.UTurnWhenPossible]: 'uTurnWhenPossible',
  [ManeuverType.EndOfDirections]: 'endOfDirections',
  [ManeuverType.SharpLeft]: 'sharpLeft',
  [ManeuverType.SharpRight]: 'sharpRight',
  [ManeuverType.SlightLeft]: 'slightLeft',
  [ManeuverType.SlightRight]: 'slightRight',
  [ManeuverType.ChangeHighway]: 'changeHighway',
  [ManeuverType.ChangeHighwayLeft]: 'changeHighwayLeft',
  [ManeuverType.ChangeHighwayRight]: 'changeHighwayRight'
}

function maneuverTypeText(value: unknown, d: Dict): string | undefined {
  const v = typeof value === 'number' ? value : undefined
  if (v == null) return undefined

  const exit = roundaboutExitNumber(v)
  if (exit !== undefined) return `${d.roundaboutExit} ${exit}`

  const key = MANEUVER_TEXT_KEY[v as ManeuverType]
  return key ? d[key] : undefined
}

function junctionTypeText(value: unknown, d: Dict): string | undefined {
  const v = typeof value === 'number' ? value : undefined
  if (v == null) return undefined
  if (v === JunctionType.Intersection) return d.junctionIntersection
  if (v === JunctionType.Roundabout) return d.junctionRoundabout
  return undefined
}

function fmtRemainingTime(totalSeconds: unknown): string | undefined {
  const s = typeof totalSeconds === 'number' ? totalSeconds : undefined
  if (s == null || !Number.isFinite(s) || s < 0) return undefined
  const total = Math.floor(s)
  const hours = Math.floor(total / 3600)
  const mins = Math.floor((total % 3600) / 60)

  if (hours > 0) {
    return `${hours}:${String(mins).padStart(2, '0')} h`
  }
  return `${mins} min`
}

// Distance display:
//   < 1 km   -> meters (e.g. 999 m)
//   1..10 km -> one decimal (e.g. 9.1 km)
//   >= 10 km -> whole km (e.g. 1065 km)
function fmtMeters(meters: unknown): string | undefined {
  const m = typeof meters === 'number' ? meters : undefined
  if (m == null || !Number.isFinite(m) || m < 0) return undefined
  if (m >= 1000) {
    const km = m / 1000
    return km < 10 ? `${km.toFixed(1)} km` : `${Math.round(km)} km`
  }
  return `${Math.round(m)} m`
}

export function translateNavigation(navi: NaviBag | null | undefined, locale: NavLocale) {
  const d = DICT[locale] ?? DICT.en
  const obj = (navi ?? {}) as Record<string, unknown>

  const sourceName = typeof obj.NaviAPPName === 'string' ? obj.NaviAPPName : undefined
  const destinationName =
    typeof obj.NaviDestinationName === 'string' ? obj.NaviDestinationName : undefined
  const currentRoadName = typeof obj.NaviRoadName === 'string' ? obj.NaviRoadName : undefined
  const afterManeuverRoadName =
    typeof obj.NaviAfterRoadName === 'string' ? obj.NaviAfterRoadName : undefined

  const timeRemainingToDestinationText = fmtRemainingTime(obj.NaviTimeToDestination)
  const distanceRemainingDisplayStringText = fmtMeters(obj.NaviDistanceToDestination)
  const remainDistanceText = fmtMeters(obj.NaviRemainDistance)

  const etaClockText =
    typeof obj.NaviETA === 'string' && obj.NaviETA.length > 0 ? obj.NaviETA : undefined

  const maneuverType = typeof obj.NaviManeuverType === 'number' ? obj.NaviManeuverType : undefined
  const maneuverText = maneuverTypeText(maneuverType, d) ?? d.unknown

  const rawTurnSide = typeof obj.NaviTurnSide === 'number' ? obj.NaviTurnSide : undefined
  const drivingSide =
    rawTurnSide === DrivingSide.Right || rawTurnSide === DrivingSide.Left ? rawTurnSide : undefined
  const drivingSideText =
    drivingSide === DrivingSide.Right
      ? d.drivingSideRight
      : drivingSide === DrivingSide.Left
        ? d.drivingSideLeft
        : undefined

  const junctionType = typeof obj.NaviJunctionType === 'number' ? obj.NaviJunctionType : undefined
  const junctionText = junctionTypeText(junctionType, d)

  const routeGuidanceState = typeof obj.NaviStatus === 'number' ? obj.NaviStatus : undefined
  const maneuverState = typeof obj.NaviOrderType === 'number' ? obj.NaviOrderType : undefined
  const turnAngle = typeof obj.NaviTurnAngle === 'number' ? obj.NaviTurnAngle : undefined

  return {
    SourceName: sourceName,
    DestinationName: destinationName,
    CurrentRoadName: currentRoadName,
    AfterManeuverRoadName: afterManeuverRoadName,

    TimeRemainingToDestinationText: timeRemainingToDestinationText,
    DistanceRemainingDisplayStringText: distanceRemainingDisplayStringText,
    RemainDistanceText: remainDistanceText,
    EtaClockText: etaClockText,

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

/** Line shown while no guidance is active. */
export function navIdleText(locale: NavLocale): string {
  return DICT[locale].noRoute
}
