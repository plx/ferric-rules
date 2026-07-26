; Harness for telefonica-clips/branches/64x/test_suite/bigbug.clp
; Detected constructs: defglobal: ?*x*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-ff875311ca35cdb8a657b274dabcfb1db36f70e0c3f31ddbfe83a4594509c2d7-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-ff875311ca35cdb8a657b274dabcfb1db36f70e0c3f31ddbfe83a4594509c2d7|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ff875311ca35cdb8a657b274dabcfb1db36f70e0c3f31ddbfe83a4594509c2d7|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ff875311ca35cdb8a657b274dabcfb1db36f70e0c3f31ddbfe83a4594509c2d7|COMPLETE" crlf))
