;; find-fact returns an empty multifield when no match exists.
;; Level: boundary
;; Covers: queries, find-fact-empty
(deftemplate item (slot value))
(defrule probe =>
    (printout t (length$ (find-fact ((?f item)) TRUE)) crlf))
