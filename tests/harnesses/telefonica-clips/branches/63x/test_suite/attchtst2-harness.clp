; Harness for telefonica-clips/branches/63x/test_suite/attchtst2.clp
; Detected constructs: deftemplate: a, b, c, d, e, f, g, h
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-9bef2acaf70bffc72747e0998283d3754228a2b77bd2d49e0cd8701f6c43d5c5-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-9bef2acaf70bffc72747e0998283d3754228a2b77bd2d49e0cd8701f6c43d5c5|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9bef2acaf70bffc72747e0998283d3754228a2b77bd2d49e0cd8701f6c43d5c5|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9bef2acaf70bffc72747e0998283d3754228a2b77bd2d49e0cd8701f6c43d5c5|COMPLETE" crlf))
