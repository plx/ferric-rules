; Harness for telefonica-clips/branches/65x/test_suite/bigbug.clp
; Detected constructs: defglobal: ?*x*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-59221802efed8183a608c45368a998208b393f8d42d44ce2a623fd0382cb428d-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-59221802efed8183a608c45368a998208b393f8d42d44ce2a623fd0382cb428d|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-59221802efed8183a608c45368a998208b393f8d42d44ce2a623fd0382cb428d|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-59221802efed8183a608c45368a998208b393f8d42d44ce2a623fd0382cb428d|COMPLETE" crlf))
