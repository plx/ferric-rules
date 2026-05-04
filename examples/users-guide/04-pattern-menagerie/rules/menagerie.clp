(deftemplate task (slot id) (slot done (default FALSE)))
(deftemplate ticket (slot severity) (slot status))

;; not — fires when nothing matches
(defrule report-no-tasks
    (start)
    (not (task))
    =>
    (printout t "queue is empty" crlf))

;; exists — fires once when at least one match exists
(defrule have-work
    (exists (task (done FALSE)))
    =>
    (printout t "work pending" crlf))

;; forall — fires when every task is done (vacuously true when none exist)
(defrule all-complete
    (ready)
    (forall (task (id ?i)) (task (id ?i) (done TRUE)))
    =>
    (printout t "everything done" crlf))

;; NCC — negate a conjunction
(defrule no-pending-deadline
    (now)
    (not (and (task (id ?i)) (deadline ?i)))
    =>
    (printout t "no pending deadlines" crlf))

;; constraint connectives inside a pattern
(defrule escalate
    (ticket (severity ~low) (status open|in-progress))
    =>
    (printout t "needs attention" crlf))
