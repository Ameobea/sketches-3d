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
 * @interface StrippedPlayer
 */
export interface StrippedPlayer {
    /**
     * 
     * @type {string}
     * @memberof StrippedPlayer
     */
    readonly id?: string | null;
    /**
     * 
     * @type {string}
     * @memberof StrippedPlayer
     */
    username: string | null;
    /**
     * 
     * @type {Date}
     * @memberof StrippedPlayer
     */
    accountCreated?: Date;
    /**
     * 
     * @type {Date}
     * @memberof StrippedPlayer
     */
    lastLoggedIn?: Date;
}

/**
 * Check if a given object implements the StrippedPlayer interface.
 */
export function instanceOfStrippedPlayer(value: object): value is StrippedPlayer {
    if (!('username' in value) || value['username'] === undefined) return false;
    return true;
}

export function StrippedPlayerFromJSON(json: any): StrippedPlayer {
    return StrippedPlayerFromJSONTyped(json, false);
}

export function StrippedPlayerFromJSONTyped(json: any, ignoreDiscriminator: boolean): StrippedPlayer {
    if (json == null) {
        return json;
    }
    return {
        
        'id': json['id'] == null ? undefined : json['id'],
        'username': json['username'],
        'accountCreated': json['accountCreated'] == null ? undefined : (new Date(json['accountCreated'])),
        'lastLoggedIn': json['lastLoggedIn'] == null ? undefined : (new Date(json['lastLoggedIn'])),
    };
}

export function StrippedPlayerToJSON(json: any): StrippedPlayer {
    return StrippedPlayerToJSONTyped(json, false);
}

export function StrippedPlayerToJSONTyped(value?: Omit<StrippedPlayer, 'id'> | null, ignoreDiscriminator: boolean = false): any {
    if (value == null) {
        return value;
    }

    return {
        
        'username': value['username'],
        'accountCreated': value['accountCreated'] == null ? undefined : ((value['accountCreated']).toISOString()),
        'lastLoggedIn': value['lastLoggedIn'] == null ? undefined : ((value['lastLoggedIn']).toISOString()),
    };
}

