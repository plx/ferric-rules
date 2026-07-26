; Harness for telefonica-clips/branches/65x/test_suite/attchtst4.clp
; Detected constructs: deftemplate: a, b, c, d, e, f, g, h
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-a9cadda806a9637f6c575c24ca8d3e95c05ef9619ada4993ec028e5197f94465-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-a9cadda806a9637f6c575c24ca8d3e95c05ef9619ada4993ec028e5197f94465|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-a9cadda806a9637f6c575c24ca8d3e95c05ef9619ada4993ec028e5197f94465|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-a9cadda806a9637f6c575c24ca8d3e95c05ef9619ada4993ec028e5197f94465|COMPLETE" crlf))
