;; fact-slot-value reads a declared single slot.
;; Level: basic
;; Covers: queries, fact-slot-value-single
(deftemplate sample (slot value))
(deffacts seed (sample (value 17)))
(defrule probe ?f <- (sample (value ?value)) => (printout t (fact-slot-value ?f value) crlf))
