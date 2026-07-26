; Harness for telefonica-clips/branches/63x/clipsdotnet/RouterWPFExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-45896a46ceb33ffa804987be1aae3988e579035d4e01a073787c51b03dbc250e-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-45896a46ceb33ffa804987be1aae3988e579035d4e01a073787c51b03dbc250e|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-45896a46ceb33ffa804987be1aae3988e579035d4e01a073787c51b03dbc250e|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-45896a46ceb33ffa804987be1aae3988e579035d4e01a073787c51b03dbc250e|COMPLETE" crlf))
