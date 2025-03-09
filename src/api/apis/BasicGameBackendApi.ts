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


import * as runtime from '../runtime';
import type {
  Play,
  PlayerLogin,
  StrippedMap,
  StrippedPlayer,
} from '../models/index';
import {
    PlayFromJSON,
    PlayToJSON,
    PlayerLoginFromJSON,
    PlayerLoginToJSON,
    StrippedMapFromJSON,
    StrippedMapToJSON,
    StrippedPlayerFromJSON,
    StrippedPlayerToJSON,
} from '../models/index';

export interface AddPlayRequest {
    mapId: string;
    timeLength: number;
}

export interface CreatePlayerRequest {
    playerLogin: PlayerLogin;
}

export interface GetLeaderboardByIndexRequest {
    mapId: string;
    startIndex: number;
    endIndex: number;
}

export interface GetMapRequest {
    mapId: string;
}

export interface GetPlaysRequest {
    playerId?: string;
}

export interface LoginRequest {
    playerLogin: PlayerLogin;
}

/**
 * 
 */
export class BasicGameBackendApi extends runtime.BaseAPI {

    /**
     */
    async addPlayRaw(requestParameters: AddPlayRequest, initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<runtime.ApiResponse<void>> {
        if (requestParameters['mapId'] == null) {
            throw new runtime.RequiredError(
                'mapId',
                'Required parameter "mapId" was null or undefined when calling addPlay().'
            );
        }

        if (requestParameters['timeLength'] == null) {
            throw new runtime.RequiredError(
                'timeLength',
                'Required parameter "timeLength" was null or undefined when calling addPlay().'
            );
        }

        const queryParameters: any = {};

        if (requestParameters['mapId'] != null) {
            queryParameters['mapId'] = requestParameters['mapId'];
        }

        if (requestParameters['timeLength'] != null) {
            queryParameters['timeLength'] = requestParameters['timeLength'];
        }

        const headerParameters: runtime.HTTPHeaders = {};

        const response = await this.request({
            path: `/play`,
            method: 'POST',
            headers: headerParameters,
            query: queryParameters,
        }, initOverrides);

        return new runtime.VoidApiResponse(response);
    }

    /**
     */
    async addPlay(requestParameters: AddPlayRequest, initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<void> {
        await this.addPlayRaw(requestParameters, initOverrides);
    }

    /**
     */
    async createPlayerRaw(requestParameters: CreatePlayerRequest, initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<runtime.ApiResponse<void>> {
        if (requestParameters['playerLogin'] == null) {
            throw new runtime.RequiredError(
                'playerLogin',
                'Required parameter "playerLogin" was null or undefined when calling createPlayer().'
            );
        }

        const queryParameters: any = {};

        const headerParameters: runtime.HTTPHeaders = {};

        headerParameters['Content-Type'] = 'application/json';

        const response = await this.request({
            path: `/register`,
            method: 'POST',
            headers: headerParameters,
            query: queryParameters,
            body: PlayerLoginToJSON(requestParameters['playerLogin']),
        }, initOverrides);

        return new runtime.VoidApiResponse(response);
    }

    /**
     */
    async createPlayer(requestParameters: CreatePlayerRequest, initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<void> {
        await this.createPlayerRaw(requestParameters, initOverrides);
    }

    /**
     */
    async getLeaderboardByIndexRaw(requestParameters: GetLeaderboardByIndexRequest, initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<runtime.ApiResponse<Array<Play>>> {
        if (requestParameters['mapId'] == null) {
            throw new runtime.RequiredError(
                'mapId',
                'Required parameter "mapId" was null or undefined when calling getLeaderboardByIndex().'
            );
        }

        if (requestParameters['startIndex'] == null) {
            throw new runtime.RequiredError(
                'startIndex',
                'Required parameter "startIndex" was null or undefined when calling getLeaderboardByIndex().'
            );
        }

        if (requestParameters['endIndex'] == null) {
            throw new runtime.RequiredError(
                'endIndex',
                'Required parameter "endIndex" was null or undefined when calling getLeaderboardByIndex().'
            );
        }

        const queryParameters: any = {};

        if (requestParameters['mapId'] != null) {
            queryParameters['mapId'] = requestParameters['mapId'];
        }

        if (requestParameters['startIndex'] != null) {
            queryParameters['startIndex'] = requestParameters['startIndex'];
        }

        if (requestParameters['endIndex'] != null) {
            queryParameters['endIndex'] = requestParameters['endIndex'];
        }

        const headerParameters: runtime.HTTPHeaders = {};

        const response = await this.request({
            path: `/leaderboard`,
            method: 'GET',
            headers: headerParameters,
            query: queryParameters,
        }, initOverrides);

        return new runtime.JSONApiResponse(response, (jsonValue) => jsonValue.map(PlayFromJSON));
    }

    /**
     */
    async getLeaderboardByIndex(requestParameters: GetLeaderboardByIndexRequest, initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<Array<Play>> {
        const response = await this.getLeaderboardByIndexRaw(requestParameters, initOverrides);
        return await response.value();
    }

    /**
     */
    async getMapRaw(requestParameters: GetMapRequest, initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<runtime.ApiResponse<any>> {
        if (requestParameters['mapId'] == null) {
            throw new runtime.RequiredError(
                'mapId',
                'Required parameter "mapId" was null or undefined when calling getMap().'
            );
        }

        const queryParameters: any = {};

        if (requestParameters['mapId'] != null) {
            queryParameters['mapId'] = requestParameters['mapId'];
        }

        const headerParameters: runtime.HTTPHeaders = {};

        const response = await this.request({
            path: `/map`,
            method: 'GET',
            headers: headerParameters,
            query: queryParameters,
        }, initOverrides);

        if (this.isJsonMime(response.headers.get('content-type'))) {
            return new runtime.JSONApiResponse<any>(response);
        } else {
            return new runtime.TextApiResponse(response) as any;
        }
    }

    /**
     */
    async getMap(requestParameters: GetMapRequest, initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<any> {
        const response = await this.getMapRaw(requestParameters, initOverrides);
        return await response.value();
    }

    /**
     */
    async getMapsRaw(initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<runtime.ApiResponse<Array<StrippedMap>>> {
        const queryParameters: any = {};

        const headerParameters: runtime.HTTPHeaders = {};

        const response = await this.request({
            path: `/maps`,
            method: 'GET',
            headers: headerParameters,
            query: queryParameters,
        }, initOverrides);

        return new runtime.JSONApiResponse(response, (jsonValue) => jsonValue.map(StrippedMapFromJSON));
    }

    /**
     */
    async getMaps(initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<Array<StrippedMap>> {
        const response = await this.getMapsRaw(initOverrides);
        return await response.value();
    }

    /**
     */
    async getPlayerRaw(initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<runtime.ApiResponse<StrippedPlayer>> {
        const queryParameters: any = {};

        const headerParameters: runtime.HTTPHeaders = {};

        const response = await this.request({
            path: `/player`,
            method: 'GET',
            headers: headerParameters,
            query: queryParameters,
        }, initOverrides);

        return new runtime.JSONApiResponse(response, (jsonValue) => StrippedPlayerFromJSON(jsonValue));
    }

    /**
     */
    async getPlayer(initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<StrippedPlayer> {
        const response = await this.getPlayerRaw(initOverrides);
        return await response.value();
    }

    /**
     */
    async getPlaysRaw(requestParameters: GetPlaysRequest, initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<runtime.ApiResponse<Array<Play>>> {
        const queryParameters: any = {};

        if (requestParameters['playerId'] != null) {
            queryParameters['playerId'] = requestParameters['playerId'];
        }

        const headerParameters: runtime.HTTPHeaders = {};

        const response = await this.request({
            path: `/plays`,
            method: 'GET',
            headers: headerParameters,
            query: queryParameters,
        }, initOverrides);

        return new runtime.JSONApiResponse(response, (jsonValue) => jsonValue.map(PlayFromJSON));
    }

    /**
     */
    async getPlays(requestParameters: GetPlaysRequest = {}, initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<Array<Play>> {
        const response = await this.getPlaysRaw(requestParameters, initOverrides);
        return await response.value();
    }

    /**
     */
    async logOutPlayerRaw(initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<runtime.ApiResponse<void>> {
        const queryParameters: any = {};

        const headerParameters: runtime.HTTPHeaders = {};

        const response = await this.request({
            path: `/logout`,
            method: 'GET',
            headers: headerParameters,
            query: queryParameters,
        }, initOverrides);

        return new runtime.VoidApiResponse(response);
    }

    /**
     */
    async logOutPlayer(initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<void> {
        await this.logOutPlayerRaw(initOverrides);
    }

    /**
     */
    async loginRaw(requestParameters: LoginRequest, initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<runtime.ApiResponse<void>> {
        if (requestParameters['playerLogin'] == null) {
            throw new runtime.RequiredError(
                'playerLogin',
                'Required parameter "playerLogin" was null or undefined when calling login().'
            );
        }

        const queryParameters: any = {};

        const headerParameters: runtime.HTTPHeaders = {};

        headerParameters['Content-Type'] = 'application/json';

        const response = await this.request({
            path: `/login`,
            method: 'POST',
            headers: headerParameters,
            query: queryParameters,
            body: PlayerLoginToJSON(requestParameters['playerLogin']),
        }, initOverrides);

        return new runtime.VoidApiResponse(response);
    }

    /**
     */
    async login(requestParameters: LoginRequest, initOverrides?: RequestInit | runtime.InitOverrideFunction): Promise<void> {
        await this.loginRaw(requestParameters, initOverrides);
    }

}
