#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionGetResponse {
    #[serde(skip)]
    pub blockchain_id: String,
    #[serde(rename = "type")]
    pub action_type: BlinkActionType,
    pub icon: String,
    pub title: String,
    pub description: String,
    pub label: String,
    pub disabled: bool,
    #[serde(skip_serializing_if = "LinkActions::is_empty")]
    pub links: LinkActions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ActionError>,
}

impl<'r> response::Responder<'r, 'static> for ActionGetResponse {
    fn respond_to(self, req: &'r Request<'_>) -> response::Result<'static> {
        Response::build_from(Json::respond_to(Json(&self), req)?)
            .raw_header("x-blockchain-ids", self.blockchain_id)
            .ok()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BlinkActionType {
    Action,
    Completed,
}

impl Default for BlinkActionType {
    fn default() -> Self {
        Self::Action
    }
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkActions {
    pub actions: Vec<LinkedAction>,
}

impl LinkActions {
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

impl From<Vec<LinkedAction>> for LinkActions {
    fn from(actions: Vec<LinkedAction>) -> Self {
        Self { actions }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedAction {
    pub label: String,
    pub href: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ActionParameter>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionParameter {
    #[serde(rename = "type", skip_serializing_if = "String::is_empty")]
    pub parameter_type: String,
    pub name: String,
    pub label: String,
    pub required: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ActionParameterOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionParameterOption {
    pub label: String,
    pub value: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionError {
    pub message: String,
}

impl From<String> for ActionError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionPostRequest<'a, T = Option<()>> {
    pub account: &'a str,
    pub signature: Option<&'a str>,
    pub data: T,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionPostResponse {
    #[serde(skip)]
    pub blockchain_id: String,
    pub transaction: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<ActionPostLinks>,
}

impl<'r> response::Responder<'r, 'static> for ActionPostResponse {
    fn respond_to(self, req: &'r Request<'_>) -> response::Result<'static> {
        Response::build_from(Json::respond_to(Json(&self), req)?)
            .raw_header("x-blockchain-ids", self.blockchain_id)
            .ok()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionPostLinks {
    pub next: NextAction,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum NextAction {
    Post { href: String },
}

// blink specific requests
#[derive(Deserialize, FromForm)]
#[serde(rename_all = "camelCase")]
pub struct MerchItemBlinkData<'a> {
    pub size: Option<&'a str>,
    #[serde(borrow)]
    pub email: Cow<'a, str>,
    #[serde(borrow)]
    pub address: Cow<'a, str>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NftActionBlinkData {
    pub price: Option<f64>,
}
