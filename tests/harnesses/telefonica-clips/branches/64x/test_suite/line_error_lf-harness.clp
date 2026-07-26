; Harness for telefonica-clips/branches/64x/test_suite/line_error_lf.clp
; Detected constructs: deffacts: points; deftemplate: point
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-9b0f4d9b57c8d4ecf3e6ddd2f4baecaa2d19504bf772e0b2d216f75e879513f3-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-9b0f4d9b57c8d4ecf3e6ddd2f4baecaa2d19504bf772e0b2d216f75e879513f3|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9b0f4d9b57c8d4ecf3e6ddd2f4baecaa2d19504bf772e0b2d216f75e879513f3|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9b0f4d9b57c8d4ecf3e6ddd2f4baecaa2d19504bf772e0b2d216f75e879513f3|COMPLETE" crlf))
