; Harness for fawkes-robotics/src/plugins/clips-executive/clips/wm-template-facts.clp
; Detected constructs: deffunction: deftemplate-remaining-slots/2, value-to-type-pair/1, values-to-type-pairs/1, type-cast/2, slots-to-multifield/2, template-fact-slots-to-key-vals/2, assert-template-wm-fact/3, template-fact-str-from-wm/2
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-9974b2db99acc32d2a0e4bb5f7692c1d9611c0f2b0f9b74e70fd4d4f34554c5e-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-9974b2db99acc32d2a0e4bb5f7692c1d9611c0f2b0f9b74e70fd4d4f34554c5e|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9974b2db99acc32d2a0e4bb5f7692c1d9611c0f2b0f9b74e70fd4d4f34554c5e|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9974b2db99acc32d2a0e4bb5f7692c1d9611c0f2b0f9b74e70fd4d4f34554c5e|COMPLETE" crlf))
