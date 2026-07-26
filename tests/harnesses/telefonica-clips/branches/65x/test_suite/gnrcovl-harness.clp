; Harness for telefonica-clips/branches/65x/test_suite/gnrcovl.clp
; Detected constructs: defglobal: ?*success*; deffunction: alt-str-cat/1, print-result/2, testit/0; defmethod: sym-cat
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-12b6d49f94a55676f56ebbaafeec39bebc62086891bd41ccbe138c7794cdf25e-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-12b6d49f94a55676f56ebbaafeec39bebc62086891bd41ccbe138c7794cdf25e|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-12b6d49f94a55676f56ebbaafeec39bebc62086891bd41ccbe138c7794cdf25e|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-12b6d49f94a55676f56ebbaafeec39bebc62086891bd41ccbe138c7794cdf25e|COMPLETE" crlf))
