; RH-CORE-013: depth strategy orders new dependent activations against queued work.
(deffacts seed (item 1) (item 2))
(defrule item (item ?n) => (printout t "item " ?n crlf) (assert (follow ?n)))
(defrule follow (follow ?n) => (printout t "follow " ?n crlf) (assert (result ?n)))
