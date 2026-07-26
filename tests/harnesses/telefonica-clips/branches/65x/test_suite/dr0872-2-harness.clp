; Harness for telefonica-clips/branches/65x/test_suite/dr0872-2.clp
; Detected constructs: defmethod: foo
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-67d1855f834cc308632e0617b330cb916c77952f1cf58181f91e363c6601ef53-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-67d1855f834cc308632e0617b330cb916c77952f1cf58181f91e363c6601ef53|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-67d1855f834cc308632e0617b330cb916c77952f1cf58181f91e363c6601ef53|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-67d1855f834cc308632e0617b330cb916c77952f1cf58181f91e363c6601ef53|COMPLETE" crlf))
