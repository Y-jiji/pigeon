use quote::ToTokens;

use crate::*;

pub trait RulesImplBuild {
    fn rules_impl_build(&self) -> Result<TokenStream>;
    fn rules_body_build(&self, fmt: &Fmt) -> Result<TokenStream>;
}

impl RulesImplBuild for Builder {
    fn rules_body_build(&self, fmt: &Fmt) -> Result<TokenStream> {
        let _crate = parse_str::<Ident>(CRATE).unwrap();
        Ok(match fmt {
            Fmt::Space => quote! {
                let end = Self::space(input, end)?;
            },
            Fmt::Token { token } => quote! {
                let end = if input[end..].starts_with(#token) {
                    end + #token.len()
                } else { Err(())? };
            },
            Fmt::Symbol { typ, group, .. } => quote! {
                let end = <#typ as ParseImpl<#group, ERROR>>::parse_impl(input, end, trace, stack)?;
            },
            Fmt::RegExp { regex, .. } => {
                let regex = format!("^({regex})");
                quote! {
                    let end = {
                        let begin = end;
                        static REGEX: #_crate::LazyLock<#_crate::Regex> = #_crate::LazyLock::new(|| #_crate::Regex::new(#regex).unwrap());
                        let Some(mat) = REGEX.find(&input[begin..]) else {
                            Err(())?
                        };
                        let mat = mat.as_str();
                        let end = end + mat.len();
                        stack.push(#_crate::Tag { rule: 0, span: begin..end });
                        end
                    };
                }
            },
            Fmt::SeqExp { children, .. } => {
                let mut body = TokenStream::new();
                for (child, flag) in children {
                    let child = child.iter()
                        .map(|fmt| self.rules_body_build(fmt))
                        .try_fold(TokenStream::new(), |mut a, b| { a.extend(b?); Result::Ok(a) })?;
                    body.extend(match flag {
                        Flag::Null => quote! {
                            let end = { #child; end }; 
                        },
                        Flag::Repeat => quote! {
                            let end = {
                                let mut end = end;
                                loop {
                                    // Use a closure to wrap `Err(...)?` to prevent exiting outer function. 
                                    let size = stack.len();
                                    if let Ok(end_) = (|| -> Result<usize, ()> { #child; Ok(end) })() {
                                        end = end_;
                                        count += 1;
                                    } else {
                                        stack.resize_with(size, || unreachable!());
                                        break end 
                                    }
                                }
                            };
                        },
                        Flag::OrNot => quote! {
                            let end = {
                                let size = stack.len();
                                // Use a closure to wrap `Err(...)?` to prevent exiting outer function. 
                                if let Ok(end_) = (|| -> Result<usize, ()> { #child; Ok(end) })() {
                                    count += 1;
                                    end_
                                } else {
                                    stack.resize_with(size, || unreachable!());
                                    end
                                }
                            };
                        }
                    });
                }
                quote! {
                    let end = {
                        let begin = end;
                        let mut count = 0usize;
                        #body;
                        stack.push(#_crate::Tag { rule: count, span: begin..end });
                        end
                    };
                }
            }
        })
    }
    fn rules_impl_build(&self) -> Result<TokenStream> {
        // The merged output stream
        let mut output = TokenStream::new();
        // For each rule, build a Rule<N> trait
        for rule in 0..self.rules.len() {
            // Add every hole in every rule
            let mut body = TokenStream::new();
            for fmt in &self.rules[rule].exprs {
                body.extend(self.rules_body_build(fmt)?);
            }
            // Add prepare identities
            let _crate = parse_str::<Ident>(CRATE).unwrap();
            let this = &self.ident;
            let generics = &self.generics.params;
            let comma = generics.to_token_stream().into_iter().last().map(|x| x.to_string() == ",").unwrap_or(false);
            let generics = 
                if !comma && !generics.is_empty() { quote! { #generics, } }
                else                              { quote! { #generics  } };
            let variant = &self.rules[rule].ident;
            // Add trace if trace presents
            let trace_prolog = if self.rules[rule].trace { quote! {
                println!("phase 1 prologue {}::{}", stringify!(#this), stringify!(#variant));
            } } else { quote! {} };
            let trace_epilog = if self.rules[rule].trace { quote! {
                println!("phase 1 epilogue {}::{}", stringify!(#this), stringify!(#variant));
            } } else { quote! {} };
            // Build Rule<N, ERROR>
            output.extend(quote! {
                impl<#generics const ERROR: bool> #_crate::RuleImpl<#rule, ERROR> for #this<#generics> 
                    where Self: #_crate::Space,
                {
                    #[inline]
                    fn rule_impl(
                        input: &str, end: usize, last: usize,
                        trace: &mut Vec<(usize, usize)>,
                        stack: &mut Vec<#_crate::Tag>,
                    ) -> Result<usize, ()> {
                        #trace_prolog
                        let size = stack.len();
                        let rule = <Self as #_crate::Num>::num(#rule);
                        let begin = end;
                        let mut inner = || -> Result<usize, ()> {
                            #body
                            stack.push(#_crate::Tag { rule, span: begin..end });
                            return Ok(end);
                        };
                        match inner() {
                            Ok(end) if end > last => {
                                #trace_epilog
                                Ok(end)
                            },
                            Err(()) | Ok(..) => {
                                #trace_epilog
                                stack.resize_with(size, || unreachable!()); 
                                Err(())
                            }
                        }
                    }
                }
            })
        }
        // The final output
        Ok(output)
    }
}