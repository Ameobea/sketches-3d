/* tslint:disable */
/* eslint-disable */
/**
 * BasicGameBackend
 * No description provided (generated by Openapi Generator https://github.com/openapitools/openapi-generator)
 *
 * The version of the OpenAPI document: 1.0
 * 
 *
 * NOTE: This class is auto generated by OpenAPI Generator (https://openapi-generator.tech).
 * https://openapi-generator.tech
 * Do not edit the class manually.
 */

import { mapValues } from '../runtime';
/**
 * 
 * @export
 * @interface Play
 */
export interface Play {
    /**
     * 
     * @type {string}
     * @memberof Play
     */
    readonly id?: string | null;
    /**
     * 
     * @type {string}
     * @memberof Play
     */
    playerId?: string | null;
    /**
     * 
     * @type {string}
     * @memberof Play
     */
    playerUserName?: string | null;
    /**
     * 
     * @type {string}
     * @memberof Play
     */
    mapId: string | null;
    /**
     * 
     * @type {number}
     * @memberof Play
     */
    playLength: number;
    /**
     * 
     * @type {Date}
     * @memberof Play
     */
    timeSubmitted: Date;
}

/**
 * Check if a given object implements the Play interface.
 */
export function instanceOfPlay(value: object): value is Play {
    if (!('mapId' in value) || value['mapId'] === undefined) return false;
    if (!('playLength' in value) || value['playLength'] === undefined) return false;
    if (!('timeSubmitted' in value) || value['timeSubmitted'] === undefined) return false;
    return true;
}

export function PlayFromJSON(json: any): Play {
    return PlayFromJSONTyped(json, false);
}

export function PlayFromJSONTyped(json: any, ignoreDiscriminator: boolean): Play {
    if (json == null) {
        return json;
    }
    return {
        
        'id': json['id'] == null ? undefined : json['id'],
        'playerId': json['playerId'] == null ? undefined : json['playerId'],
        'playerUserName': json['playerUserName'] == null ? undefined : json['playerUserName'],
        'mapId': json['mapId'],
        'playLength': json['playLength'],
        'timeSubmitted': (new Date(json['timeSubmitted'])),
    };
}

export function PlayToJSON(json: any): Play {
    return PlayToJSONTyped(json, false);
}

export function PlayToJSONTyped(value?: Omit<Play, 'id'> | null, ignoreDiscriminator: boolean = false): any {
    if (value == null) {
        return value;
    }

    return {
        
        'playerId': value['playerId'],
        'playerUserName': value['playerUserName'],
        'mapId': value['mapId'],
        'playLength': value['playLength'],
        'timeSubmitted': ((value['timeSubmitted']).toISOString()),
    };
}

