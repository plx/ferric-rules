;; fact-slot-names preserves template declaration order.
;; Level: basic
;; Covers: queries, fact-slot-names-template
(deftemplate sample (slot zeta) (multislot alpha) (slot middle))
(deffacts seed (sample (zeta z) (alpha a) (middle m)))
(defrule probe ?f <- (sample (zeta ?zeta)) => (printout t (fact-slot-names ?f) crlf))
