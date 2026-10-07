;; A pending seed query keeps its template in use, so a later redefinition in the same source fails load.
;; Level: interaction
;; Covers: assertion-expression, deffacts, deftemplate, compile-time-validation
(deftemplate item (slot n))
(deffacts seed (probe ready (if FALSE then (any-factp ((?f item)) TRUE) else FALSE)))
(deftemplate item (slot replacement))
