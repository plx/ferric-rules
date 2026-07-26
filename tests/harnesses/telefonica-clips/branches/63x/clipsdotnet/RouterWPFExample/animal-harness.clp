; Harness for telefonica-clips/branches/63x/clipsdotnet/RouterWPFExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-56c77a70b4f6983863c03aaed15cad8b47a5050fb80f9ac333bd737ae818986e-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-56c77a70b4f6983863c03aaed15cad8b47a5050fb80f9ac333bd737ae818986e|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-56c77a70b4f6983863c03aaed15cad8b47a5050fb80f9ac333bd737ae818986e|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-56c77a70b4f6983863c03aaed15cad8b47a5050fb80f9ac333bd737ae818986e|COMPLETE" crlf))
