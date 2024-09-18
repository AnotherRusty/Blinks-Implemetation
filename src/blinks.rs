extern crate foster_data_layer;
extern crate rocket;

use chrono::Utc;
use rocket::serde::json::{json, Json};
use std::collections::HashMap;

use crate::{
    editions::create_print,
    maddies::{get_ship_station_timestamp, ship_station_request},
};
use foster_data_layer::{
    calculate_payment_shares, create_merch_order_and_order_products,
    create_user_from_wallet_and_email, get_merch_order_info, get_merch_product_details,
    get_single_nft_response, get_sol_to_usd_rate, get_user_by_wallet_id, mint_single_nft,
    models::{
        ActionGetResponse, ActionParameter, ActionParameterOption, ActionPostLinks,
        ActionPostRequest, ActionPostResponse, BlinkActionType, ErrorResponse, FulfillmentType,
        LinkedAction, MerchItemBlinkData, MerchProductWithCurrentSupply, NewMerchOrder,
        NewSingleNft, NextAction, NftActionBlinkData, PrintEditionRequest, ShipStationAddress,
        ShipStationOrder, ShipStationOrderItem, ShipStationOrderItemOption, SingleNftResponse,
        UpdateMerchOrder, Weight,
    },
    update_order, MERCH_PAYMENT_ADDRESS,
};
use foster_solana::{
    assert_minimum_balance, blinks::create_merch_blink_transaction, get_nft_from_das,
    get_solana_network, lamports_to_sol, sol_to_lamports, validate_blink_payment,
    validate_public_key, SOL_SYMBOL,
};

macro_rules! uri {
    ($path: expr) => {
        rocket::uri!("/blinks", $path)
    };
}

pub const MAINNET_BLOCKCHAIN_ID: &str = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";
pub const DEVNET_BLOCKCHAIN_ID: &str = "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1";

pub fn get_blockchain_id() -> String {
    match get_solana_network().as_ref() {
        "mainnet" => MAINNET_BLOCKCHAIN_ID.to_string(),
        _ => DEVNET_BLOCKCHAIN_ID.to_string(),
    }
}

#[get("/<_artist>/merch/<item_id>")]
pub async fn blink_merch_item_get(_artist: &str, item_id: i32) -> ActionGetResponse {
    let blockchain_id = get_blockchain_id();
    let product = match get_merch_product_details(item_id) {
        Ok(product) => product,
        Err(e) => {
            return ActionGetResponse {
                blockchain_id,
                title: "Invalid Product".to_string(),
                // TODO: set url for invalid product
                // icon: "".to_string(),
                description: format!("Could not find product with id {item_id}"),
                label: "Buy".to_string(),
                disabled: true,
                error: Some(e.into()),
                ..ActionGetResponse::default()
            };
        }
    };

    let mut parameters = vec![
        ActionParameter {
            parameter_type: "email".to_string(),
            name: "email".to_string(),
            label: "Email".to_string(),
            required: true,
            ..ActionParameter::default()
        },
        ActionParameter {
            parameter_type: "textarea".to_string(),
            name: "address".to_string(),
            label: "Shipping Address".to_string(),
            required: true,
            ..ActionParameter::default()
        },
    ];

    let fulfillment_type = product
        .fulfillment_type
        .parse::<FulfillmentType>()
        .unwrap_or_else(|e| {
            panic!(
                "could not parse as FulfillmentType: {}: {e}",
                product.fulfillment_type
            )
        });
    // TODO: check if size is needed, and which sizes are supported
    if matches!(fulfillment_type, FulfillmentType::Foster) {
        parameters.splice(
            0..0,
            [ActionParameter {
                parameter_type: "select".to_string(),
                name: "size".to_string(),
                label: "Size".to_string(),
                required: true,
                options: vec![
                    ActionParameterOption {
                        label: "Small".to_string(),
                        value: "S".to_string(),
                    },
                    ActionParameterOption {
                        label: "Medium".to_string(),
                        value: "M".to_string(),
                    },
                    ActionParameterOption {
                        label: "Large".to_string(),
                        value: "L".to_string(),
                    },
                    ActionParameterOption {
                        label: "Extra Large".to_string(),
                        value: "XL".to_string(),
                    },
                    ActionParameterOption {
                        label: "2XL".to_string(),
                        value: "XXL".to_string(),
                    },
                    ActionParameterOption {
                        label: "3XL".to_string(),
                        value: "XXXL".to_string(),
                    },
                ],
                ..ActionParameter::default()
            }],
        );
    }

    // + 2% slippage
    let usd_per_sol = get_sol_to_usd_rate().await.unwrap_or_default() / 1.02;
    // fixed $15 for shipping
    let usd_amount = (product.selling_price + 1500) as f64 / 100.0;
    let sol_amount = usd_amount / usd_per_sol;

    // TODO: maybe add selector to choose payment token between sol or usdc
    ActionGetResponse {
        blockchain_id,
        // TODO: add url for products with missing image
        icon: get_image_for_product(&product).unwrap_or_default(),
        title: product.name,
        description: product.description,
        label: "Buy".to_string(),
        links: vec![LinkedAction {
            label: format!("Buy for {SOL_SYMBOL}{sol_amount:.2} | ${usd_amount:.2}"),
            href: format!(
                "/v1/blinks/{_artist}/merch/{item_id}/?size={{size}}&email={{email}}&address={{address}}",
            ),
            parameters,
        }]
        .into(),
        ..ActionGetResponse::default()
    }
}

fn get_image_for_product(product: &MerchProductWithCurrentSupply) -> Option<String> {
    let fulfillment_type = product
        .fulfillment_type
        .parse::<FulfillmentType>()
        .unwrap_or_else(|e| {
            panic!(
                "could not parse as FulfillmentType: {}: {e}",
                product.fulfillment_type
            )
        });
    match fulfillment_type {
        FulfillmentType::Foster => product
            .options
            .get("addons")?
            .get(0)?
            .get("mockup_url")?
            .as_str()
            .map(|s| s.to_string()),
        FulfillmentType::User => product
            .options
            .get("product_images")?
            .get(0)?
            .as_str()
            .map(|s| s.to_string()),
    }
}

#[post(
    "/<_artist>/merch/<item_id>?<options..>",
    format = "application/json",
    data = "<request>",
    rank = 2
)]
pub async fn blink_merch_item_post(
    _artist: &str,
    item_id: i32,
    options: MerchItemBlinkData<'_>,
    request: Json<ActionPostRequest<'_>>,
) -> Result<ActionPostResponse, ErrorResponse> {
    let MerchItemBlinkData {
        size,
        email,
        address,
    } = &options;

    let product = get_merch_product_details(item_id)?;
    if let Some(supply) = product.supply {
        if (product.current_supply as i32) >= supply {
            return Err(format!(
                "product {} has sold out; max supply: {supply}",
                product.name
            )
            .into());
        }
    }
    let now = Utc::now().naive_utc();
    if let Some(sale_start_at) = product.sale_start_at {
        if now < sale_start_at {
            return Err(format!("product {} sale starts at {sale_start_at}", product.name).into());
        }
    }
    if let Some(sale_end_at) = product.sale_end_at {
        if now > sale_end_at {
            return Err(format!("product {} sale ended at {sale_end_at}", product.name).into());
        }
    }

    let seller_amount = product.selling_price - product.foster_amount;
    // fixed $15 for shipping
    let foster_amount = product.foster_amount + 1500;
    let usd_amount = seller_amount + foster_amount;

    let user_pubkey = validate_public_key(request.account)?;
    let user = match get_user_by_wallet_id(user_pubkey) {
        Some(user) => user,
        None => create_user_from_wallet_and_email(user_pubkey, Some(email)),
    };

    let seller_shares = calculate_payment_shares(
        vec![(product.user_id, seller_amount)],
        vec![(MERCH_PAYMENT_ADDRESS.to_string(), foster_amount)],
    )?;

    // + 2% slippage
    let usd_per_sol = get_sol_to_usd_rate().await.unwrap_or_default() / 1.02;
    let seller_shares_lamports = seller_shares
        .iter()
        .map(|(address, usd_amount)| {
            (
                address.clone(),
                sol_to_lamports((*usd_amount as f64) / (100.0 * usd_per_sol)),
            )
        })
        .collect::<HashMap<_, _>>();

    let total_lamports = seller_shares_lamports.values().sum::<u64>();
    assert_minimum_balance(request.account, total_lamports + 20_000).await?;

    let (order, _) = create_merch_order_and_order_products(
        NewMerchOrder {
            user_id: user.id,
            status: "created-blink",
            shipping_address: Some(&json!({
                "rawAddress": address
            })),
            // TODO: remove fulfillment type
            fulfillment_type: "",
            external_order_id: None,
            total_amount_usd: &usd_amount,
            total_amount_token: &(seller_shares_lamports.values().sum::<u64>() as i32),
            payment_splits: &serde_json::to_value(seller_shares)
                .map_err(|e| format!("could not serialize payment splits: {e}"))?,
            payment_method: "SOL",
            transaction_id: None,
        },
        vec![(product.id, 1, None)],
    )?;

    let transaction = create_merch_blink_transaction(user_pubkey, seller_shares_lamports).await?;

    Ok(ActionPostResponse {
        blockchain_id: get_blockchain_id(),
        transaction,
        message: Some(
            [
                Some(format!("Placing Order #{}: {}", order.id, product.name)),
                size.map(|size| size.to_string()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" | "),
        ),
        links: Some(ActionPostLinks {
            next: NextAction::Post {
                href: uri!(blink_merch_item_checkout_post(
                    order_id = order.id,
                    email = email.as_ref(),
                    size = size.as_deref()
                ))
                .to_string(),
            },
        }),
    })
}

#[post("/merch/<order_id>/checkout?<email>&<size>", data = "<request>")]
pub async fn blink_merch_item_checkout_post(
    order_id: i32,
    email: &str,
    size: Option<&str>,
    request: Json<ActionPostRequest<'_>>,
) -> Result<ActionGetResponse, ErrorResponse> {
    let payment_reference = request
        .signature
        .as_ref()
        .ok_or_else(|| "invalid request: missing signature".to_string())?;

    let order = get_merch_order_info(order_id)
        .map_err(|e| format!("could not find order with id {order_id}: {e}"))?;
    if let Some(tx) = order.transaction_id {
        return Err(format!("order {order_id} already paid by tx {tx}").into());
    }

    validate_blink_payment(payment_reference, order).await?;

    let product = get_merch_product_details(order.items[0].id)?;
    let product_image = get_image_for_product(&product).unwrap_or_default();

    let user = get_user_by_wallet_id(request.account)
        .ok_or_else(|| format!("could not find user with account {}", request.account))?;

    // TODO: parse address
    let address = ShipStationAddress {
        name: user.username.unwrap_or(user.wallet_id),
        street1: Some(
            order
                .shipping_address
                .and_then(|address| Some(address.get("rawAddress")?.as_str()?.to_string()))
                .unwrap_or("could not get street1 of address".to_string()),
        ),
        city: Some("Blink City".to_string()),
        ..ShipStationAddress::default()
    };

    let product_addons = &product.options.get("addons").and_then(|e| e.as_array());

    let network = get_solana_network();
    let order_date = get_ship_station_timestamp(&Utc::now());
    let ssorder = ship_station_request(reqwest::Method::POST, "/orders/createorder")
        .json(&ShipStationOrder {
            order_id: 0,
            order_number: format!("foster/studio/{network}/{order_id}"),
            order_key: "".to_string(),
            order_date: order_date.clone(),
            payment_date: Some(order_date),
            order_status: "awaiting_shipment".to_string(),
            customer_id: None,
            customer_email: user.email.unwrap_or_else(|| email.to_string()),

            bill_to: Some(address.clone()),
            ship_to: Some(address),

            items: vec![ShipStationOrderItem {
                order_item_id: 0,
                line_item_key: product.id.to_string(),
                sku: None,
                name: product.name.clone(),
                image_url: Some(product_image.clone()),
                weight: Weight {
                    value: 0.0,
                    units: "ounces".to_string(),
                },
                quantity: 1,
                unit_price: product.selling_price as f64 / 100.0,
                tax_amount: None,
                options: [
                    Some(ShipStationOrderItemOption {
                        name: "type".to_string(),
                        value: product.fulfillment_type.clone(),
                    }),
                    Some(ShipStationOrderItemOption {
                        name: "fosterUrl".to_string(),
                        value: format!(
                            "{}/_/merch/{}",
                            match network.as_str() {
                                "mainnet" => "https://fostermarketplace.app",
                                _ => "https://devnet.fostermarketplace.app",
                            },
                            product.id
                        ),
                    }),
                    Some(ShipStationOrderItemOption {
                        name: "assetUrl".to_string(),
                        value: product_addons
                            .map(|addons| {
                                addons
                                    .iter()
                                    .filter_map(|addon| addon.get("raw_url")?.as_str())
                                    .collect::<Vec<_>>()
                                    .join(",")
                            })
                            .unwrap_or_default(),
                    }),
                    Some(ShipStationOrderItemOption {
                        name: "mockupUrl".to_string(),
                        value: product_addons
                            .map(|addons| {
                                addons
                                    .iter()
                                    .filter_map(|addon| addon.get("raw_url")?.as_str())
                                    .collect::<Vec<_>>()
                                    .join(",")
                            })
                            .unwrap_or_default(),
                    }),
                    product
                        .options
                        .get("print_technique")
                        .and_then(|e| e.as_str())
                        .map(|technique| ShipStationOrderItemOption {
                            name: "technique".to_string(),
                            value: technique.to_string(),
                        }),
                    size.map(|size| ShipStationOrderItemOption {
                        name: "size".to_string(),
                        value: size.to_string(),
                    }),
                ]
                .into_iter()
                .flatten()
                .collect(),
                adjustment: false,
            }],
            amount_paid: order.total_amount_usd as f64 / 100.0,
            tax_amount: 0.0,
            shipping_amount: 15.0,

            customer_notes: "ordered via blink!".to_string(),
            internal_notes: format!(
                "assetUrl: {}",
                get_image_for_product(&product).unwrap_or_default()
            ),

            gift: false,
            gift_message: None,

            payment_method: Some(format!("blinks: tx {payment_reference}")),
            requested_shipping_service: Some("blinks".to_string()),

            weight: Weight {
                value: 0.5,
                units: "ounces".to_string(),
            },
            tag_ids: None,
        })
        .send()
        .await
        .map_err(|e| format!("failed to POST /orders/createorder: {e}"))?
        .json::<ShipStationOrder>()
        .await
        .map_err(|e| format!("failed to create order: {e}"))?;

    update_order(
        order_id,
        UpdateMerchOrder {
            external_order_id: Some(Some(ssorder.order_id)),
            transaction_id: Some(Some(payment_reference.to_string())),
            payment_method: Some("SOL".to_string()),
            // TODO: fetch payment amount from chain
            total_amount_token: Some(1),
            ..UpdateMerchOrder::default()
        },
    )?;

    Ok(ActionGetResponse {
        blockchain_id: get_blockchain_id(),
        action_type: BlinkActionType::Completed,
        title: format!("Order #{order_id}"),
        // TODO: show confetti GIF
        icon: product_image,
        description: format!(
            "Manage your order at {}/orders/{order_id}",
            match network.as_str() {
                "mainnet" => "https://fostermarketplace.app",
                _ => "https://devnet.fostermarketplace.app",
            }
        ),
        label: "Order placed successfully!".to_string(),
        disabled: true,
        ..ActionGetResponse::default()
    })
}

#[get("/nft/<token_id>")]
pub async fn blink_nft_get(token_id: &str) -> ActionGetResponse {
    let das_nft_future = get_nft_from_das(token_id);

    let blockchain_id = get_blockchain_id();
    let nft = match get_single_nft_response(token_id) {
        Ok(nft) => nft,
        Err(e) => {
            return ActionGetResponse {
                blockchain_id,
                title: "Invalid NFT".to_string(),
                // TODO: set url for invalid nft
                // icon: "".to_string(),
                description: format!("Could not find nft with address {token_id}"),
                label: "Buy".to_string(),
                disabled: true,
                error: Some(e.into()),
                ..ActionGetResponse::default()
            };
        }
    };
    let artist_name = get_user_by_wallet_id(&nft.minter_id)
        .and_then(|artist| artist.username)
        .unwrap_or(nft.minter_id.clone());

    let mut links = vec![];

    let usd_per_sol = get_sol_to_usd_rate().await.unwrap_or_default();

    // if there is a listing, allow buying
    if let Some(listing) = &nft.listing {
        let sol_amount = listing.list_price.parse::<f64>().unwrap_or_default();
        let usd_amount = sol_amount * usd_per_sol;
        links.push(LinkedAction {
            label: format!("Buy now for {SOL_SYMBOL}{sol_amount:.2} (~${usd_amount:.2})"),
            href: uri!(blink_nft_post(
                token_id = token_id,
                action = "buy",
                price = _
            ))
            .to_string(),
            parameters: vec![],
        });
    }
    // for auctions, allow placing a minimum or custom bid
    else if let Some(auction_response) = &nft.auction {
        // minimum bid
        let minimum_bid = if let Some(highest_bid) = &auction_response.highest_bid {
            // TODO: save min_bid_increment to db
            highest_bid.amount.parse::<f64>().unwrap_or_default() + 0.01
        } else {
            let mut reserve_price = auction_response
                .auction
                .reserve_price
                .parse::<f64>()
                .unwrap_or_default();
            if reserve_price == 0.0 {
                reserve_price = 0.1;
            }
            reserve_price
        };
        let usd_amount = minimum_bid * usd_per_sol;
        links.push(LinkedAction {
            label: format!("Place bid for {SOL_SYMBOL}{minimum_bid:.2} (~${usd_amount:.2})"),
            href: uri!(blink_nft_post(
                token_id = token_id,
                action = "bid",
                price = Some(minimum_bid)
            ))
            .to_string(),
            parameters: vec![],
        });

        // custom bid
        links.push(LinkedAction {
            label: "Place bid".to_string(),
            href: uri!(blink_nft_post(
                token_id = token_id,
                action = "bid",
                price = _
            ))
            .to_string(),
            parameters: vec![ActionParameter {
                parameter_type: "number".to_string(),
                name: "price".to_string(),
                label: "Custom amount".to_string(),
                required: true,
                min: Some(minimum_bid),
                ..ActionParameter::default()
            }],
        });
    }
    // for listed master edition, allow buying a print
    else if let Some(master_edition) = &nft.master_edition {
        let mut lamport_amount = master_edition.price.parse::<u64>().unwrap_or_default();
        if let Some(merch_product) = &master_edition.merch_product {
            // +2% slippage
            lamport_amount +=
                ((merch_product.foster_amount as f64 * 1e7 * 1.02) / usd_per_sol) as u64;
        }

        let sol_amount = lamports_to_sol(lamport_amount);
        let usd_amount = sol_amount * usd_per_sol;

        links.push(LinkedAction {
            label: format!("Buy for {SOL_SYMBOL}{sol_amount:.2} (~${usd_amount:.2})"),
            href: uri!(blink_nft_post(
                token_id = token_id,
                action = "buy-print",
                price = _
            ))
            .to_string(),
            parameters: vec![],
        });
    }
    // finally, allow placing an offer on the nft
    else {
        links.push(LinkedAction {
            label: "Place offer".to_string(),
            href: uri!(blink_nft_post(
                token_id = token_id,
                action = "place-offer",
                price = _
            ))
            .to_string(),
            parameters: vec![ActionParameter {
                parameter_type: "number".to_string(),
                name: "price".to_string(),
                label: "Custom amount".to_string(),
                required: true,
                min: Some(0.01),
                ..ActionParameter::default()
            }],
        });
    }

    // TODO: include product price in sol and usd in the action label
    ActionGetResponse {
        blockchain_id,
        // TODO: add url for products with missing image
        icon: get_image_for_nft(&nft).unwrap_or_default(),
        title: nft.nft_name,
        // TODO: fetch nft description from chain
        description: [
            match das_nft_future.await {
                Ok(das_nft) => das_nft.result.content.metadata.description,
                Err(e) => format!("DAS error: {e}"),
            },
            "".to_string(),
            format!("nft by {}", artist_name),
        ]
        .join("\n"),
        links: links.into(),
        ..ActionGetResponse::default()
    }
}

fn get_image_for_nft(nft: &SingleNftResponse) -> Option<String> {
    let image_url = match nft.asset_type.as_ref() {
        _ if nft.asset_type.as_str().starts_with("video") => nft.cover_image_url.clone(),
        _ if nft.asset_type.as_str().starts_with("audio") => nft.cover_image_url.clone(),
        "vr" => nft.cover_image_url.clone(),
        _ => Some(nft.asset_url.clone()),
    };
    image_url.map(|url| format!("https://cdn.helius-rpc.com/cdn-cgi/image/quality=75/{url}"))
}

#[post(
    "/nft/<token_id>/<action>?<price>",
    format = "application/json",
    data = "<request>",
    rank = 1
)]
pub async fn blink_nft_post(
    token_id: &str,
    action: &str,
    price: Option<f64>,
    request: Json<ActionPostRequest<'_, Option<NftActionBlinkData>>>,
) -> Result<ActionPostResponse, ErrorResponse> {
    let NftActionBlinkData {
        price: request_price,
    } = match &request.data {
        Some(data) => data,
        None => &NftActionBlinkData::default(),
    };

    let effective_price = price.and(*request_price);

    // TODO: implement transaction creation for all action types
    let response = match action {
        "buy-print" => {
            let prints = create_print(
                token_id,
                Json(PrintEditionRequest {
                    buyer: request.account,
                    count: Some(1),
                    editions: None,
                }),
            )
            .await?;
            let print_info = &prints[0];

            ActionPostResponse {
                blockchain_id: get_blockchain_id(),
                transaction: print_info.transaction.clone(),
                message: Some(format!(
                    "Minting Print Edition #{}",
                    print_info.edition_number
                )),
                links: Some(ActionPostLinks {
                    next: NextAction::Post {
                        href: uri!(blink_nft_index_print_post(
                            token_id = &print_info.edition_mint,
                        ))
                        .to_string(),
                    },
                }),
            }
        }
        "buy" | "bid" | "place-offer" => {
            return Err(format!("action not implemented: {action}").into())
        }
        _ => return Err(format!("unknown blink action: {action}").into()),
    };

    Ok(response)
}

#[post("/nft/index-print/<token_id>", data = "<request>", rank = 0)]
pub async fn blink_nft_index_print_post(
    token_id: &str,
    request: Json<ActionPostRequest<'_>>,
) -> Result<ActionGetResponse, ErrorResponse> {
    let das_nft = get_nft_from_das(token_id)
        .await
        .map_err(|e| format!("DAS error: {e}"))?;

    let parent_nft = match das_nft.result.supply.master_edition_mint {
        Some(parent_nft) => get_single_nft_response(&parent_nft),
        None => Err(format!("error: nft is not a print {token_id}")),
    }?;

    let new_nft = NewSingleNft {
        owner_id: request.account,
        token_id,
        minter_id: request.account,
        collection_id: parent_nft.collection_id.clone(),
        nft_name: &parent_nft.nft_name,
        minted_on_foster: true,
        views: 0,
        likes: 0,
        shares: 0,
        saves: 0,
        categories: &parent_nft.categories,
        asset_url: &parent_nft.asset_url,
        asset_type: &parent_nft.asset_type,
        cover_image_url: parent_nft.cover_image_url.as_deref(),
        royalties: &parent_nft.royalties,
        parent_nft: Some(&parent_nft.token_id),
        max_supply: das_nft
            .result
            .supply
            .print_max_supply
            .map(|max_supply| max_supply.into()),
        edition: das_nft
            .result
            .supply
            .edition_number
            .unwrap_or_default()
            .into(),
    };

    // when editions are minted, notify artist
    foster_notification::mint_edition(&new_nft);
    mint_single_nft(new_nft);

    Ok(ActionGetResponse {
        blockchain_id: get_blockchain_id(),
        // TODO: add url for products with missing image
        icon: get_image_for_nft(&parent_nft).unwrap_or_default(),
        title: parent_nft.nft_name,
        description: das_nft.result.content.metadata.description,
        label: "NFT bought successfully!".to_string(),
        ..ActionGetResponse::default()
    })
}
