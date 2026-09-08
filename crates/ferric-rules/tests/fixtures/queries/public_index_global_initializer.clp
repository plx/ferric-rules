;; Issue #329: user-visible fact assertion indices.
(deftemplate item (slot value))
(defglobal ?*address* = FALSE)
(deffacts seed (item (value 10)))
(defrule capture =>
  (do-for-fact ((?f item)) TRUE (bind ?*address* ?f)))
;; INITIALIZE AFTER CAPTURE
(defglobal ?*index* = (fact-index ?*address*))
(defrule report => (printout t ?*index* crlf))
