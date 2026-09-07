;; any-factp is FALSE for an empty template.
;; Level: boundary
;; Covers: queries, any-factp-empty
(deftemplate item (slot value))
(defrule probe =>
    (printout t (any-factp ((?f item)) TRUE) crlf))
