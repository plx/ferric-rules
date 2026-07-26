; Harness for clips-executive/cx_plugins/config_plugin/clips/ff-config.clp
; Detected constructs: deftemplate: confval
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-c8549472ead01c3815212e5bfa81da5a1cb4485a3dd44a3991129b910ae5506c-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-c8549472ead01c3815212e5bfa81da5a1cb4485a3dd44a3991129b910ae5506c|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-c8549472ead01c3815212e5bfa81da5a1cb4485a3dd44a3991129b910ae5506c|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-c8549472ead01c3815212e5bfa81da5a1cb4485a3dd44a3991129b910ae5506c|COMPLETE" crlf))
