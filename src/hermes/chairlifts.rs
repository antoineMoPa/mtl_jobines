//! Preserve the original chalet lift and any explicitly authored side lifts.
use crate::dynamic_world::SpawnPoint;
pub(super) fn read(program: &opencity::Program) -> Result<Vec<[SpawnPoint; 2]>, String> {
    let mut lifts = match (
        SpawnPoint::from_named_program(program, "chairlift_base")?,
        SpawnPoint::from_named_program(program, "chairlift_top")?,
    ) {
        (Some(base), Some(top)) => vec![[base, top]],
        (None, None) => Vec::new(),
        _ => return Err("chairlift requires both chairlift_base and chairlift_top".into()),
    };
    for statement in &program.statements {
        let opencity::Statement::Call(call) = statement else {
            continue;
        };
        if call.name != "chairlift" {
            continue;
        }
        if call.args.len() != 4 {
            return Err("chairlift expects base-x,base-y,top-x,top-y".into());
        }
        let values = call
            .args
            .iter()
            .map(|a| match a {
                opencity::Argument::Number(n) => n.parse::<f32>().ok().filter(|v| v.is_finite()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .ok_or("chairlift expects four finite coordinates")?;
        lifts.push([
            SpawnPoint {
                point: [values[0], values[1]],
                heading: None,
            },
            SpawnPoint {
                point: [values[2], values[3]],
                heading: None,
            },
        ]);
    }
    if lifts.len() > 8 {
        return Err("at most eight ski hill chairlifts are supported".into());
    }
    Ok(lifts)
}
