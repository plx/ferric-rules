;; any-factp is TRUE when at least one fact matches.
;; Level: basic
;; Covers: queries, any-factp-match
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule probe =>
    (printout t (any-factp ((?f item)) TRUE) crlf))