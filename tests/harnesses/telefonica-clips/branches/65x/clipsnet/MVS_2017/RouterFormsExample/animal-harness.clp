; Harness for telefonica-clips/branches/65x/clipsnet/MVS_2017/RouterFormsExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-74cf527135291180dd00e110c89b7333ce4816e4c168524d29bf2c722f37ebbf-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-74cf527135291180dd00e110c89b7333ce4816e4c168524d29bf2c722f37ebbf|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-74cf527135291180dd00e110c89b7333ce4816e4c168524d29bf2c722f37ebbf|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-74cf527135291180dd00e110c89b7333ce4816e4c168524d29bf2c722f37ebbf|COMPLETE" crlf))
