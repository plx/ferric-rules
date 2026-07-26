; Harness for clips-executive/extensions/pddl/cx_pddl_bringup/clips/cx_pddl_bringup/deftemplate-overrides.clp
; Detected constructs: deftemplate: pddl-action
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-9d0e1ae328a8d3c6d2b93865c7b19fa8bf6bd684e76249a473db02119a7574ee-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-9d0e1ae328a8d3c6d2b93865c7b19fa8bf6bd684e76249a473db02119a7574ee|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9d0e1ae328a8d3c6d2b93865c7b19fa8bf6bd684e76249a473db02119a7574ee|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9d0e1ae328a8d3c6d2b93865c7b19fa8bf6bd684e76249a473db02119a7574ee|COMPLETE" crlf))
