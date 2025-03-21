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
 * @interface StrippedMap
 */
export interface StrippedMap {
    /**
     * 
     * @type {string}
     * @memberof StrippedMap
     */
    id: string | null;
    /**
     * 
     * @type {number}
     * @memberof StrippedMap
     */
    authorTime?: number;
    /**
     * 
     * @type {number}
     * @memberof StrippedMap
     */
    sPlusTime?: number;
    /**
     * 
     * @type {number}
     * @memberof StrippedMap
     */
    sTime?: number;
    /**
     * 
     * @type {number}
     * @memberof StrippedMap
     */
    aTime?: number;
    /**
     * 
     * @type {number}
     * @memberof StrippedMap
     */
    bTime?: number;
}

/**
 * Check if a given object implements the StrippedMap interface.
 */
export function instanceOfStrippedMap(value: object): value is StrippedMap {
    if (!('id' in value) || value['id'] === undefined) return false;
    return true;
}

export function StrippedMapFromJSON(json: any): StrippedMap {
    return StrippedMapFromJSONTyped(json, false);
}

export function StrippedMapFromJSONTyped(json: any, ignoreDiscriminator: boolean): StrippedMap {
    if (json == null) {
        return json;
    }
    return {
        
        'id': json['id'],
        'authorTime': json['authorTime'] == null ? undefined : json['authorTime'],
        'sPlusTime': json['sPlusTime'] == null ? undefined : json['sPlusTime'],
        'sTime': json['sTime'] == null ? undefined : json['sTime'],
        'aTime': json['aTime'] == null ? undefined : json['aTime'],
        'bTime': json['bTime'] == null ? undefined : json['bTime'],
    };
}

export function StrippedMapToJSON(json: any): StrippedMap {
    return StrippedMapToJSONTyped(json, false);
}

export function StrippedMapToJSONTyped(value?: StrippedMap | null, ignoreDiscriminator: boolean = false): any {
    if (value == null) {
        return value;
    }

    return {
        
        'id': value['id'],
        'authorTime': value['authorTime'],
        'sPlusTime': value['sPlusTime'],
        'sTime': value['sTime'],
        'aTime': value['aTime'],
        'bTime': value['bTime'],
    };
}

