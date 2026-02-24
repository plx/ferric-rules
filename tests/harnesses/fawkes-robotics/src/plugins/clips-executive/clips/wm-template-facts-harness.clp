; Harness for fawkes-robotics/src/plugins/clips-executive/clips/wm-template-facts.clp
; Detected constructs: deffunction: deftemplate-remaining-slots/2, value-to-type-pair/1, values-to-type-pairs/1, type-cast/2, slots-to-multifield/2, template-fact-slots-to-key-vals/2, assert-template-wm-fact/3, template-fact-str-from-wm/2
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
