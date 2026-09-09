;; Issue #329: user-visible fact assertion indices.
(deftemplate item (slot value))
(defglobal ?*saved* = FALSE)
(defglobal ?*replacement* = FALSE)
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule capture =>
  (do-for-fact ((?f item)) (= (fact-slot-value ?f value) 10)
    (bind ?*saved* ?f)
    (printout t "saved:" (fact-index ?f) crlf)))
;; RESUME RULE
;; Run capture, host-retract item10, then host-assert item40 before loading this.
(defrule resumed =>
  (printout t "stale:" (fact-index ?*saved*) crlf)
  (do-for-fact ((?f item)) (= (fact-slot-value ?f value) 20)
    (printout t "20:" (fact-index ?f) crlf))
  (do-for-fact ((?f item)) (= (fact-slot-value ?f value) 30)
    (printout t "30:" (fact-index ?f) crlf))
  (do-for-fact ((?f item)) (= (fact-slot-value ?f value) 40)
    (bind ?*replacement* ?f)
    (printout t "40:" (fact-index ?f) crlf)))
