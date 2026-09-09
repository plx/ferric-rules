;; find-all-facts returns one address per matching fact.
;; Level: basic
;; Covers: queries, find-all-facts-count
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule probe =>
    (printout t (length$ (find-all-facts ((?f item)) TRUE)) crlf))