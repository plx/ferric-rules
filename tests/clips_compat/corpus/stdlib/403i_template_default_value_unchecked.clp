;; Slot constraints apply when a fact is asserted, not to a queried default.
(deffunction f () 20)
(deftemplate item (slot x (range 0 10) (default-dynamic (f))))
(defrule probe =>
  (printout t (deftemplate-slot-defaultp item x) " "
    (deftemplate-slot-default-value item x) crlf))
